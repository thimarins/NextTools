//! Rule-based recommendations from a finished copy's real metrics.
//! Documented rules (docs/DECISIONS.md ADR-040), no external AI, never
//! applied automatically: the user decides.

use nexttool_core::ErrorKind;
use serde::Serialize;

use crate::job::{CopyOptions, VerifyLevel};
use crate::result::CopyStats;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Recommendation {
    pub id: &'static str,
    pub title: String,
    /// Measured facts that triggered the rule.
    pub evidence: String,
    pub suggestion: String,
    /// "baixa" | "média" | "alta"
    pub confidence: &'static str,
}

pub struct CopyFacts<'a> {
    pub stats: &'a CopyStats,
    pub elapsed_ms: u64,
    pub options: &'a CopyOptions,
    pub dominant_error: Option<ErrorKind>,
    pub remote_source: bool,
    pub remote_dest: bool,
    /// The activity monitor flagged a persistent slowdown during the copy.
    pub slowdown_seen: bool,
}

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;

fn fmt_mb(b: f64) -> String {
    format!("{:.1} MB", b / MIB as f64)
}

pub fn recommend(f: &CopyFacts) -> Vec<Recommendation> {
    let s = f.stats;
    let mut out = Vec::new();
    let files = s.files_copied.max(1);
    let avg = s.bytes_copied / files;
    let secs = (f.elapsed_ms as f64 / 1000.0).max(0.001);
    let fps = s.files_copied as f64 / secs;
    let remote = f.remote_source || f.remote_dest;

    // R1: many small files → metadata-bound; more threads usually help (bounded on NAS).
    if s.files_copied >= 10_000 && avg < 64 * KIB && f.options.threads < 16 {
        let target = if remote { "12–16" } else { "16–32" };
        out.push(Recommendation {
            id: "small-files-threads",
            title: "Muitos arquivos pequenos".into(),
            evidence: format!("{} arquivos, média de {:.1} KB, {:.0} arquivos/s com {} threads", s.files_copied, avg as f64 / KIB as f64, fps, f.options.threads),
            suggestion: format!("Com arquivos pequenos, o tempo é dominado por operações de metadados. Teste {target} threads na próxima execução{}.", if remote { " (em NAS, aumente aos poucos para não sobrecarregar o servidor)" } else { "" }),
            confidence: "média",
        });
    }
    // R2: very large files → unbuffered I/O.
    if s.files_copied > 0 && avg >= 256 * MIB && !f.options.unbuffered {
        out.push(Recommendation {
            id: "large-files-unbuffered",
            title: "Arquivos grandes".into(),
            evidence: format!("média de {} por arquivo", fmt_mb(avg as f64)),
            suggestion: "Para arquivos muito grandes, a cópia sem buffer (opção /J) costuma reduzir o uso de memória do sistema e pode aumentar a taxa. Avalie num teste.".into(),
            confidence: "baixa",
        });
    }
    // R3: large files over the network without restartable mode.
    if remote && avg >= 1024 * MIB && !f.options.restartable {
        out.push(Recommendation {
            id: "large-remote-restartable",
            title: "Arquivos grandes pela rede".into(),
            evidence: format!("média de {} por arquivo em origem/destino de rede", fmt_mb(avg as f64)),
            suggestion: "Ative o modo reiniciável (perfil Migração ou /Z): se a conexão cair, o arquivo continua de onde parou em vez de recomeçar.".into(),
            confidence: "média",
        });
    }
    // R4: locked files.
    if f.dominant_error == Some(ErrorKind::Locked) || (s.files_failed > 0 && f.dominant_error == Some(ErrorKind::Locked)) {
        out.push(Recommendation {
            id: "locked-files",
            title: "Arquivos em uso".into(),
            evidence: format!("{} arquivo(s) com falha, causa predominante: arquivo bloqueado por outro programa", s.files_failed),
            suggestion: "Agende a cópia para um horário sem uso (Agendar…) ou feche os programas que usam esses arquivos; depois retome — o que já foi copiado não é copiado de novo.".into(),
            confidence: "alta",
        });
    }
    // R5: many retries / network errors → reduce pressure.
    if (f.dominant_error == Some(ErrorKind::Network) || f.dominant_error == Some(ErrorKind::Timeout)) && f.options.threads > 4 {
        out.push(Recommendation {
            id: "network-reduce-threads",
            title: "Instabilidade de rede".into(),
            evidence: format!("erros de rede/timeout com {} threads; {} retentativas observadas", f.options.threads, s.retries),
            suggestion: "Reduza as threads (ex.: 4–8) e use o perfil Cópia segura: menos conexões simultâneas tendem a ser mais estáveis em links instáveis.".into(),
            confidence: "média",
        });
    }
    // R6: persistent slowdown observed by the monitor.
    if f.slowdown_seen {
        out.push(Recommendation {
            id: "slowdown",
            title: "Queda persistente de velocidade".into(),
            evidence: "o monitor registrou taxa recente abaixo de 30% da média da própria tarefa por mais de 3 minutos".into(),
            suggestion: "Verifique: rede congestionada, NAS/disco sobrecarregado, antivírus inspecionando os arquivos, outras tarefas no mesmo destino. A causa não pode ser afirmada só pelas métricas.".into(),
            confidence: "baixa",
        });
    }
    // R7: ACL/owner migration without content verification.
    if (f.options.flags.security || f.options.flags.owner) && f.options.verify == VerifyLevel::Basic {
        out.push(Recommendation {
            id: "migration-verify",
            title: "Migração sem verificação".into(),
            evidence: "cópia com permissões/proprietário e verificação básica".into(),
            suggestion: "Para migrações, use pelo menos a verificação Padrão (existência, tamanho e data); a Rigorosa compara o conteúdo por hash.".into(),
            confidence: "alta",
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::CopyProfile;

    fn facts<'a>(stats: &'a CopyStats, o: &'a CopyOptions) -> CopyFacts<'a> {
        CopyFacts { stats, elapsed_ms: 60_000, options: o, dominant_error: None, remote_source: false, remote_dest: false, slowdown_seen: false }
    }

    #[test]
    fn rules_fire_only_with_evidence() {
        let o = CopyProfile::Safe.options();
        // Healthy medium-size copy: no recommendation.
        let s = CopyStats { files_copied: 500, bytes_copied: 500 * 5 * MIB, ..Default::default() };
        assert!(recommend(&facts(&s, &o)).is_empty());
        // 100k small files with 8 threads → R1.
        let s = CopyStats { files_copied: 100_000, bytes_copied: 100_000 * 2 * KIB, ..Default::default() };
        let r = recommend(&facts(&s, &o));
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id, "small-files-threads");
        assert!(r[0].evidence.contains("100000 arquivos"));
        // Large files over the network → R2 + R3.
        let s = CopyStats { files_copied: 10, bytes_copied: 10 * 2048 * MIB, ..Default::default() };
        let mut f = facts(&s, &o);
        f.remote_dest = true;
        let ids: Vec<_> = recommend(&f).iter().map(|r| r.id).collect();
        assert_eq!(ids, vec!["large-files-unbuffered", "large-remote-restartable"]);
        // Locked + network + slowdown.
        let s = CopyStats { files_copied: 10, bytes_copied: 10, files_failed: 2, ..Default::default() };
        let mut f = facts(&s, &o);
        f.dominant_error = Some(ErrorKind::Locked);
        assert!(recommend(&f).iter().any(|r| r.id == "locked-files"));
        f.dominant_error = Some(ErrorKind::Network);
        f.slowdown_seen = true;
        let ids: Vec<_> = recommend(&f).iter().map(|r| r.id).collect();
        assert!(ids.contains(&"network-reduce-threads") && ids.contains(&"slowdown"));
        // Migration with basic verify.
        let mut m = CopyProfile::ServerMigration.options();
        m.verify = VerifyLevel::Basic;
        let s = CopyStats::default();
        assert!(recommend(&facts(&s, &m)).iter().any(|r| r.id == "migration-verify"));
    }
}
