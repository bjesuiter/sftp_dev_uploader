use std::{
    io::Error,
    path::{Path, PathBuf},
};

/**
 * Converts "/home/myuser/mypath/myfile.txt" to "mypath/myfile.txt", assuming base_path is "/home/myuser".
 * Also works for dir paths: "/home/myuser/mypath" => "mypath"
 *
 * @param input_path: The local absolute path to be converted to a relative path.
 * @param base_path:
 *   The base path to be used to compute the relative path.
 *   If None, the local current working directory is used (via std::env::current_dir()).
 *
 * Note: Solves the problem:
 * - local_path is /Users/myuser/myfolder/myfile.txt
 * - remote_cwd is /opt/myuser
 * => remote_path should be /opt/myuser/myfolder/myfile.txt
 * => therefore I need to compute the relative path part (myfolder/myfile.txt)
 *    and use it as remote path on top of the remote_cwd
 * => Formula: local_path - local_cwd = relative_path
 *
 * Win: upload recursive structures!
 */
pub fn compute_relative_path_from_local(
    input_path: &Path,
    base_path: Option<&Path>,
) -> Result<PathBuf, Error> {
    if input_path.is_relative() {
        return Err(Error::new(
            std::io::ErrorKind::InvalidInput,
            "input_path must be an absolute path",
        ));
    }

    // from here on: input_path is absolute
    let use_base_path =
        match base_path.map_or_else(|| std::env::current_dir(), |p| p.canonicalize()) {
            Ok(p) => p,
            Err(e) => {
                return Err(Error::new(
                    std::io::ErrorKind::NotFound,
                    format!(
                        // Error will be: No such file or directory
                        "{}\n       base_path: {}",
                        e.to_string(),
                        base_path.unwrap().to_string_lossy(),
                    ),
                ));
            }
        };

    //Strip the base_path from input_path
    match input_path.strip_prefix(&use_base_path) {
        Ok(relative_path) => {
            return Ok(relative_path.to_path_buf());
        }
        Err(e) => {
            return Err(Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "Failed to compute relative path from: 
                    input_path: {}
                    using base_path: {}
                    inner error: {}",
                    input_path.to_string_lossy(),
                    use_base_path.to_string_lossy(),
                    e.to_string()
                ),
            ));
        }
    };
}
