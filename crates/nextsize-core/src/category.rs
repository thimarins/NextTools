//! File categories by extension (documented, deterministic, no content sniffing).

pub const CATEGORIES: &[(&str, &str, &[&str])] = &[
    ("documents", "Documentos", &["doc", "docx", "odt", "rtf", "txt", "pdf", "xls", "xlsx", "ods", "csv", "ppt", "pptx", "odp", "md", "epub", "vsdx", "pub", "one"]),
    ("images", "Imagens", &["jpg", "jpeg", "png", "gif", "bmp", "tif", "tiff", "webp", "heic", "raw", "cr2", "nef", "dng", "psd", "ai", "svg", "ico"]),
    ("videos", "Vídeos", &["mp4", "mkv", "avi", "mov", "wmv", "m4v", "mpg", "mpeg", "flv", "webm", "m2ts", "vob"]),
    ("audio", "Áudio", &["mp3", "wav", "flac", "aac", "ogg", "wma", "m4a", "opus", "aiff"]),
    ("archives", "Compactados", &["zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz", "zst", "cab", "iso"]),
    ("backups", "Backups e imagens de disco", &["bak", "bkf", "vbk", "vib", "vrb", "tib", "wim", "vhd", "vhdx", "vmdk", "qcow2", "old", "backup"]),
    ("email", "E-mail", &["pst", "ost", "msg", "eml", "mbox"]),
    ("databases", "Bancos de dados", &["mdf", "ldf", "ndf", "db", "sqlite", "accdb", "mdb", "dbf", "frm", "ibd", "nxs"]),
    ("executables", "Programas e instaladores", &["exe", "dll", "msi", "msp", "sys", "jar", "apk", "appx", "msix", "cmd", "bat", "ps1"]),
    ("code", "Código-fonte", &["c", "h", "cpp", "hpp", "cs", "java", "py", "js", "ts", "tsx", "jsx", "rs", "go", "php", "rb", "sql", "json", "xml", "yaml", "yml", "html", "css"]),
    ("temporary", "Temporários e logs", &["tmp", "temp", "log", "dmp", "etl", "cache", "crdownload", "part"]),
];

/// Category id for an (already normalized, lowercase) extension.
pub fn category_of(ext: &str) -> &'static str {
    CATEGORIES.iter().find(|(_, _, exts)| exts.contains(&ext)).map(|c| c.0).unwrap_or("other")
}

pub fn label_of(id: &str) -> &'static str {
    CATEGORIES.iter().find(|c| c.0 == id).map(|c| c.1).unwrap_or("Outros")
}

/// Extensions of a category (empty for unknown ids / "other").
pub fn extensions_of(id: &str) -> &'static [&'static str] {
    CATEGORIES.iter().find(|c| c.0 == id).map(|c| c.2).unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn categories() {
        assert_eq!(category_of("pdf"), "documents");
        assert_eq!(category_of("bak"), "backups");
        assert_eq!(category_of("xyz"), "other");
        assert_eq!(label_of("videos"), "Vídeos");
        // An extension belongs to at most one category.
        let mut seen = std::collections::HashSet::new();
        for (_, _, exts) in CATEGORIES {
            for e in *exts {
                assert!(seen.insert(*e), "duplicated extension {e}");
            }
        }
    }
}
