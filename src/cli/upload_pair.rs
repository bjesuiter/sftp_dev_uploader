use std::path::PathBuf;

#[derive(Debug)]
pub struct UploadPair {
    pub source: PathBuf,
    pub target: PathBuf,
}

impl UploadPair {
    pub fn new(source: PathBuf, target: Option<PathBuf>) -> Self {
        let target = match target {
            Some(target) => target,
            None => {
                if source.is_absolute() {
                    panic!("ERROR: Target path in upload-pair must be provided when source path is absolute!");
                } else {
                    source.clone()
                }
            }
        };

        UploadPair { source, target }
    }

    pub fn from_uploadpair_string(upload_pair_string: &str) -> Self {
        let mut parts = upload_pair_string.split(":");

        let source = parts.next().unwrap().trim();
        let target = match parts.next() {
            Some(target) => Some(PathBuf::from(target.trim())),
            None => None,
        };

        UploadPair::new(PathBuf::from(source), target)
    }
}
