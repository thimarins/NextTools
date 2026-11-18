//! Alert texts. Plain language for users, technical details for admins.
//! Causes are stated only when there is evidence (error code, counters).

use nexttool_core::alert::AlertDraft;
use nexttool_core::task::{Module, TaskId};
use nexttool_core::{ErrorKind, Severity};

use crate::monitor::Diagnosis;

pub fn for_error(module: Module, task: TaskId, kind: ErrorKind, target: &str, will_retry: bool, details: String) -> AlertDraft {
    let (title, problem, cause, action, sev) = match kind {
        ErrorKind::Network => (
            "Erro de rede — origem ou destino indisponível",
            format!("A tarefa perdeu acesso a {target}."),
            Some("O sistema informou erro de rede (compartilhamento, servidor ou conexão indisponível).".to_string()),
            if will_retry {
                "A tarefa foi interrompida temporariamente para evitar falhas adicionais. O sistema tentará recuperar a conexão conforme a política configurada.".to_string()
            } else {
                "Verifique a conexão com o servidor/NAS e retome a tarefa.".to_string()
            },
            Severity::Error,
        ),
        ErrorKind::Timeout => (
            "Tempo de resposta esgotado",
            format!("{target} não respondeu dentro do tempo limite."),
            Some("O sistema operacional retornou timeout.".to_string()),
            if will_retry { "Uma nova tentativa será feita automaticamente.".to_string() } else { "Verifique a carga do servidor/NAS e retome a tarefa.".to_string() },
            Severity::Warning,
        ),
        ErrorKind::AccessDenied => (
            "Permissão negada",
            format!("O usuário atual não tem permissão em {target}."),
            Some("O sistema retornou 'acesso negado'.".to_string()),
            "Execute com um usuário que tenha acesso ou ajuste as permissões. A tarefa não será repetida automaticamente.".to_string(),
            Severity::Error,
        ),
        ErrorKind::Authentication => (
            "Falha de autenticação",
            format!("As credenciais da sessão Windows não foram aceitas por {target}."),
            Some("Logon recusado, sessão expirada ou conflito de credenciais.".to_string()),
            "Conecte-se ao compartilhamento pelo Windows (Gerenciador de Credenciais) e retome a tarefa. O NextTool não armazena senhas.".to_string(),
            Severity::Error,
        ),
        ErrorKind::DiskFull => (
            "Sem espaço em disco",
            format!("Não há espaço suficiente em {target}."),
            Some("O sistema retornou 'disco cheio' ou cota excedida.".to_string()),
            "Libere espaço e retome a tarefa. Os dados já gravados foram preservados.".to_string(),
            Severity::Critical,
        ),
        ErrorKind::Integrity => (
            "Falha de integridade na verificação",
            format!("A verificação encontrou divergências em {target}."),
            None,
            "Consulte o relatório de divergências antes de usar o destino. A tarefa não foi marcada como concluída.".to_string(),
            Severity::Critical,
        ),
        ErrorKind::CorruptState => (
            "Estado interno corrompido",
            format!("O arquivo de estado {target} estava corrompido."),
            None,
            "O arquivo foi preservado para análise e um novo foi criado. Revise as tarefas recentes.".to_string(),
            Severity::Critical,
        ),
        _ => (
            "Falha na tarefa",
            format!("A operação em {target} falhou."),
            None,
            "Consulte os detalhes técnicos e o log da tarefa.".to_string(),
            Severity::Error,
        ),
    };
    AlertDraft {
        module,
        task_id: Some(task),
        severity: sev,
        fingerprint: format!("{}.{}.{}", module.as_str(), kind.as_str(), target.to_lowercase()),
        title: title.into(),
        problem,
        probable_cause: cause,
        consequence: if will_retry { "A tarefa está aguardando nova tentativa." } else { "A tarefa não foi concluída." }.into(),
        recommended_action: action,
        recoverable: will_retry || kind.is_transient(),
        details: Some(details),
    }
}

pub fn interrupted(module: Module, task: TaskId, title: &str, child_running: bool) -> AlertDraft {
    AlertDraft {
        module,
        task_id: Some(task),
        severity: Severity::Warning,
        fingerprint: format!("{}.interrupted.{task}", module.as_str()),
        title: "Tarefa interrompida inesperadamente".into(),
        problem: format!("A tarefa \"{title}\" estava em execução quando o NextTool foi encerrado (falha, reinicialização ou queda de energia)."),
        probable_cause: None,
        consequence: "A tarefa não foi concluída. Os dados já processados foram preservados.".into(),
        recommended_action: if child_running {
            "O processo de cópia anterior ainda está em execução. Aguarde o término ou encerre-o antes de retomar, para evitar duas cópias simultâneas.".into()
        } else {
            "Revise e retome a tarefa. Arquivos já copiados/analisados não serão processados em duplicidade.".into()
        },
        recoverable: true,
        details: None,
    }
}

pub fn activity(module: Module, task: TaskId, d: &Diagnosis) -> Option<AlertDraft> {
    use crate::monitor::Activity::*;
    let (title, problem, sev) = match d.activity {
        Slow => (
            "Desempenho abaixo do esperado",
            "A tarefa apresentou redução persistente de velocidade em relação à sua própria média.",
            Severity::Info,
        ),
        NoObservableProgress => (
            "Sem progresso observável",
            "Não há progresso, atividade de I/O nem crescimento do log. Pode ser uma operação longa ou um compartilhamento sem resposta. O processo NÃO foi encerrado.",
            Severity::Warning,
        ),
        _ => return None,
    };
    Some(AlertDraft {
        module,
        task_id: Some(task),
        severity: sev,
        fingerprint: format!("{}.activity.{:?}.{task}", module.as_str(), d.activity),
        title: title.into(),
        problem: problem.into(),
        probable_cause: None,
        consequence: "A tarefa continua em execução.".into(),
        recommended_action: match d.activity {
            Slow => "Possíveis causas a verificar: rede congestionada, NAS sobrecarregado, muitos arquivos pequenos, antivírus, concorrência elevada, disco lento.".into(),
            _ => "Verifique a disponibilidade da origem/destino. Você pode pausar a tarefa e retomá-la depois.".into(),
        },
        recoverable: true,
        details: Some(format!("confiança: {:?}; evidências: {}", d.confidence, d.evidence.join("; "))),
    })
}
