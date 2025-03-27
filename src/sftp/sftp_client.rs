use ssh2::{FileStat, Session, Sftp};
use std::{
    collections::{HashMap, VecDeque},
    io::{copy, BufReader, BufWriter, Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
};

use super::local_utils::compute_relative_path_from_local;

// Custom error type for SftpClient
#[derive(Debug)]
pub enum SftpClientError {
    MissingPassword,
    MissingPubkeyPath,
    MissingPrivatekeyPath,
    OpenLocalFileError {
        path: PathBuf,
        io_error: std::io::Error,
    },
    OpenRemoteFileError {
        path: PathBuf,
        ssh2_error: ssh2::Error,
    },
    CloseRemoteFileError {
        path: PathBuf,
        ssh2_error: ssh2::Error,
    },
    RemotePathError {
        msg: String,
        path: PathBuf,
    },
    LocalPathError {
        msg: String,
        path: PathBuf,
    },
    SftpConnectionMissing {
        msg: String,
    },
    LocalToRemoteCopyError {
        local_path: PathBuf,
        remote_path: PathBuf,
        io_error: std::io::Error,
    },
    RemoteMkdirError {
        msg: String,
        path: PathBuf,
        inner_error: ssh2::Error,
    },
}

enum AuthMethod {
    PasswordBased {
        password: String,
    },
    KeyBased {
        pubkey: PathBuf,
        privatekey: PathBuf,
        /**
         * Optional: The passphrase for the ssh key
         */
        passphrase: Option<String>,
    },
}

/**
 * This is ONE ssh2 session on top of ONE TCP connection.
 * Parallel Transfers should be done by creating multiple ssh2_client instances.
 *
 * Note: the Ssh2Client struct should completely contain owned types (aka no lifetimes),
 * since it should be standalone and not depend on it's environment.
 *
 * If we want str or Path as inputs, convert them in the ::new Constructor
 */
pub struct SftpClient {
    pub uploader_name: String,
    host: String,
    port: u16,
    username: String,

    /**
     * Password or Key-based authentication details
     */
    auth_method: AuthMethod,

    /**
     * All the runtime props in one struct (Check if this works correctly)
     */
    runtime_props: RuntimeProps,
}

/**
 * Inner Struct of SftpClient
 * Stores the props needed at runtime, like the ssh2 session, the tcp stream, etc.
 */
struct RuntimeProps {
    _tcp_stream: Option<TcpStream>,
    ssh2_session: Option<Session>,
    // 2 channels, one for commands and one for file data
    // - Reason: We may need to send an exit command over the command channel
    //   while the file channel transfers some data
    // TODO: maybe delete these, since the sftp subsystem communicates entirely independent
    command_channel: Option<ssh2::Channel>,
    file_channel: Option<ssh2::Channel>,

    /**
     * The ssh2 lib sftp connection
     */
    pub sftp_connection: Option<ssh2::Sftp>,

    /**
     * Since ssh2 lib has no concept of a remote cd command,
     * we have to keep track of the remote ourselves.
     */
    remote_cwd: Option<PathBuf>,

    /**
     * Flag wether the sftp client has been closed already.
     * If not, close it when SftpClient goes out of scope (aka. is dropped).
     */
    is_closed: bool,

    /**
     * Special state for the dev-uploader I write
     */
    remote_dir_cache: HashMap<PathBuf, bool>,
}

/**
 * Close the client when it goes out of scope, if it's not closed already
 */
impl Drop for SftpClient {
    fn drop(&mut self) {
        self.close();
    }
}

impl SftpClient {
    // --------------------------------
    // Constructors on SftpClient
    // --------------------------------

    /**
     * Init a new SftpClient with public and private key-based authentication
     */
    pub fn new(
        uploader_name: &str,
        host: &str,
        port: u16,
        username: &str,
        pubkey: PathBuf,
        privatekey: PathBuf,
        passphrase: Option<String>,
    ) -> Self {
        // generate runtime props
        let runtime_props = RuntimeProps {
            _tcp_stream: None,
            ssh2_session: None,
            command_channel: None,
            file_channel: None,
            sftp_connection: None,
            remote_cwd: None,
            is_closed: false,
            remote_dir_cache: HashMap::new(),
        };

        // create the SftpClient instance and validate pubkey and privatekey availability
        let sftp_client = SftpClient {
            auth_method: AuthMethod::KeyBased {
                pubkey,
                privatekey,
                passphrase,
            },
            uploader_name: String::from(uploader_name),
            host: String::from(host),
            port,
            username: String::from(username),
            runtime_props,
        };

        sftp_client
    }

    /**
     * Init a new SftpClient with password-based authentication
     */
    pub fn with_password(
        uploader_name: &str,
        host: &str,
        port: u16,
        username: &str,
        password: &str,
    ) -> Self {
        // generate runtime props
        let runtime_props = RuntimeProps {
            _tcp_stream: None,
            ssh2_session: None,
            command_channel: None,
            file_channel: None,
            sftp_connection: None,
            remote_cwd: None,
            is_closed: false,
            remote_dir_cache: HashMap::new(),
        };

        SftpClient {
            auth_method: AuthMethod::PasswordBased {
                password: String::from(password),
            },
            uploader_name: String::from(uploader_name),
            host: String::from(host),
            port,
            username: String::from(username),
            runtime_props,
        }
    }

    // ---------------------------------
    // Getters and Setters on SftpClient
    // ---------------------------------

    pub fn session(&self) -> &Option<Session> {
        &self.runtime_props.ssh2_session
    }

    pub fn set_session(&mut self, session: Session) -> () {
        self.runtime_props.ssh2_session = Some(session);
    }

    pub fn sftp_connection(&self) -> Result<&Sftp, SftpClientError> {
        match &self.runtime_props.sftp_connection {
            Some(sftp) => Ok(sftp),
            None => {
                return Err(SftpClientError::SftpConnectionMissing {
                    msg: String::from(
                        "Sftp connection is missing, did you forget to call SftpClient::connect()?",
                    ),
                });
            }
        }
    }

    pub fn set_sftp_connection(&mut self, sftp_connection: ssh2::Sftp) -> () {
        self.runtime_props.sftp_connection = Some(sftp_connection);
    }

    pub fn remote_cwd(&self) -> Option<&Path> {
        self.runtime_props
            .remote_cwd
            .as_ref()
            .map(|pathbuf| pathbuf.as_path())
    }

    pub fn remote_cwd_as_pathbuf(&self) -> Option<PathBuf> {
        self.runtime_props.remote_cwd.clone()
    }

    pub fn set_remote_cwd(&mut self, remote_cwd: PathBuf) -> () {
        self.runtime_props.remote_cwd = Some(remote_cwd);
    }

    // -----------------------
    // Functions on SftpClient
    // -----------------------

    pub fn connect(&mut self) -> () {
        let host_and_port = format!("{}:{}", self.host, self.port);

        // STEP 1: create underlying TcpStream with timeout
        let tcp =
            TcpStream::connect(host_and_port.as_str()).expect("Unable to connect to SSH server");

        // STEP 2: create ssh session & connect it to the tcp stream
        let mut ssh_session = match Session::new() {
            Ok(sess) => sess,
            Err(_) => panic!("Failed to create SSH session"),
        };
        ssh_session.set_tcp_stream(tcp);
        ssh_session.set_compress(true);

        // STEP 2.1 set some options on the ssh session
        // match ssh_session.set_banner("sftp dev uploader - Version xxx") {
        //     Ok(_) => {}
        //     Err(e) => panic!("Failed to set banner: {}", e),
        // };

        // STEP 2.2 execute the ssh auth handshake
        match ssh_session.handshake() {
            Ok(_) => {
                // STEP 3: Authenticate the session
                // Use the user's private key for authentication or the password
                // Replace "~/.ssh/id_rsa" with the actual path to your private key if different

                match &self.auth_method {
                    AuthMethod::PasswordBased { password } => {
                        ssh_session
                            .userauth_password(self.username.as_str(), password.as_str())
                            .expect("Failed to authenticate using password");
                    }
                    AuthMethod::KeyBased {
                        pubkey,
                        privatekey,
                        passphrase,
                    } => {
                        ssh_session
                            .userauth_pubkey_file(
                                self.username.as_str(), // Replace with your SSH username
                                Some(pubkey.as_path()), // Public key path (can be None if using the default ~/.ssh/id_rsa.pub)
                                privatekey.as_path(),   // Path to private key file
                                passphrase.as_deref(), // Passphrase (if your key is not encrypted, use None)
                            )
                            .expect("Failed to authenticate using public key");
                    }
                }
            }
            Err(e) => panic!("Failed to do ssh handshake: {}", e),
        };

        if !ssh_session.authenticated() {
            panic!("Authentication failed");
        }

        // STEP 4: store authenticated session on the ssh client
        self.runtime_props.ssh2_session = Some(ssh_session);

        let channel1 = self
            .session()
            .as_ref()
            .unwrap()
            .channel_session()
            .expect("Failed to create SSH channel");
        self.runtime_props.command_channel = Some(channel1);

        let channel2 = self
            .session()
            .as_ref()
            .unwrap()
            .channel_session()
            .expect("Failed to create SSH channel");
        self.runtime_props.file_channel = Some(channel2);

        // STEP 5 create sftp connection on the existing ssh session
        self.set_sftp_connection(
            self.session()
                .as_ref()
                .unwrap()
                .sftp()
                .expect("Failed to create SFTP connection"),
        );

        // init remote sftp vars
        let initial_cwd = self.initial_pwd_remote();
        self.set_remote_cwd(initial_cwd);
    }

    pub fn exec_ssh_command(&mut self, command: &str) -> String {
        let channel = self.runtime_props.command_channel.as_mut().unwrap();

        channel
            .exec(command)
            .expect(&format!("{} {}", "Failed to execute command: ", command));

        let mut output = String::new();
        channel
            .read_to_string(&mut output)
            .expect("Failed to read command output");

        return output;
    }

    pub fn close(&mut self) -> () {
        if self.session().is_some() && !self.runtime_props.is_closed {
            self.session()
                .as_ref()
                .unwrap()
                .disconnect(None, "Bye bye", Some("en"))
                .expect("Failed to disconnect");
            self.runtime_props.is_closed = true;
        }
    }

    // SFTP Commands
    // see: https://docs.rs/ssh2/latest/ssh2/struct.Sftp.html
    // named like the commands of "sftp"-cli (normal commands are remote, l-prefixed ones are local)
    // --------------------------------------------------------

    /**
     * Basic SFTP command to get the working directory of the remote server
     *
     * (the directory where the sftp session is currently located)
     * This is very important, since the SSH2 Library does not have a way of changing the remote cwd,
     * so we have to keep track of it ourselves.
     * Using self.remote_cwd for that.
     */
    pub fn initial_pwd_remote(&self) -> PathBuf {
        let out = self
            .sftp_connection()
            .as_ref()
            .unwrap()
            .realpath(Path::new("."))
            .expect("Failed to get current working directory");

        return out;
    }

    /**
     * Pure SFTP Command to get the working directory of the remote server,
     * taking into account our own self.remote_cwd variable
     *
     * Note: This function does simply echo the value of self.remote_cwd and does not validate it.
     * Normally self.remote_cwd should contain only valid values, since the self.cd function should validate it while switching dirs.
     * If you need to explicitly validate the remote cwd, use self.pwd_with_validation().
     *
     * Note: This function takes a mutable reference of self, since it might need to set the remote cwd if it's not set yet.
     * This guarantees that the remote cwd is always set, so I can safely return a reference to it.
     * If you call this function on the outside, you may run into errors like:
     * - cannot borrow self as mutable more than once at a time
     * - cannot borrow self as mutable because it is also borrowed as immutable
     * => To solve this, run .to_path_buf() on the &Path returned by this function, which copies it and expires the mutable or immutable borrow.
     *
     * This function returns a &Path, not a &PathBuf, since it's more memory efficient to return a reference when using this function internally.
     * IDEA TODO: make this function non public, rename it to pwd_remote_internal and add another function which provides pwd_remote for external users as PathBuf.
     */
    pub fn pwd_remote(&mut self) -> &Path {
        if self.remote_cwd().is_none() {
            let initial_cwd = self.initial_pwd_remote();
            self.set_remote_cwd(initial_cwd);
        }

        self.remote_cwd().as_ref().unwrap()
    }

    /**
     * Pure SFTP Command to get the working directory of the remote server + validate it,
     * taking into account our own self.remote_cwd variable
     */
    pub fn pwd_remote_with_validation(&mut self) -> Result<&Path, String> {
        let remote_cwd = self.pwd_remote().to_path_buf();
        // mutable borrow of self ends here, since this function now owns the path_buf of remote cwd
        let conn_result = self.sftp_connection();
        let remote_stat = conn_result.unwrap().stat(remote_cwd.as_path());

        match remote_stat {
            Ok(stat) => {
                if stat.is_dir() {
                    Ok(self.pwd_remote())
                } else {
                    Err(
                        format!(
                        "Current remote working dir '{}' is not a directory - probably developer error!", remote_cwd.display()
                    ))
                }
            }
            Err(ssh2_error) => Err(format!(
                "Current remote working dir '{}' could not be validated: {}",
                remote_cwd.display(),
                ssh2_error.to_string()
            )),
        }
    }

    /**
     * List the contents of a remote directory
     * If no remote_path is given, the current working directory is used,
     * taking self.remote_cwd into account.
     */
    pub fn ls_remote(&mut self, remote_path: Option<&str>) -> Result<Vec<PathBuf>, String> {
        let remote_pathbuf = match remote_path {
            Some(remote_path) => {
                let path = Path::new(remote_path);
                self.canonicalize_remote(path)
            }
            None => self.pwd_remote().to_path_buf(),
        };

        let raw_out = match self
            .sftp_connection()
            .as_ref()
            .unwrap()
            .readdir(remote_pathbuf.as_path())
        {
            Ok(entries) => entries,
            Err(ssh2_error) => {
                return Err(format!(
                    "Failed to read directory '{}': {}",
                    remote_pathbuf.display(),
                    ssh2_error.to_string()
                ));
            }
        };

        let out = raw_out
            .into_iter()
            .map(|entry| {
                return PathBuf::from(entry.0);
            })
            .collect::<Vec<PathBuf>>();

        return Ok(out);
    }

    pub fn ls_local(&mut self, local_path: Option<&str>) -> Vec<PathBuf> {
        let path = PathBuf::from(local_path.unwrap_or("."));

        let out = std::fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.expect("Failed to read directory entry").path())
            .collect::<Vec<PathBuf>>();

        return out;
    }

    pub fn pwd_local(&mut self) -> std::io::Result<PathBuf> {
        return std::env::current_dir();
    }

    /**
     * Changes the remote cwd to the given path and takes self.remote_cwd into account
     *
     * @param new_path: relative or absolute path to the new remote directory
     * @returns the new remote cwd or an error message
     */
    pub fn cd_remote(&mut self, new_path: &str) -> Result<PathBuf, String> {
        let mut remote_pathbuf = self.canonicalize_remote(Path::new(new_path));

        // TODO: Handle potential errors
        // resolving the realpath here is important,
        // it removes ".." and "." from the path
        remote_pathbuf = match self
            .sftp_connection()
            .as_ref()
            .unwrap()
            .realpath(remote_pathbuf.as_path())
        {
            Ok(pathbuf) => pathbuf,
            Err(ssh2_error) => {
                return Err(format!(
                    "Cannot cd into '{}': {}",
                    remote_pathbuf.display(),
                    ssh2_error.to_string()
                ));
            }
        };

        // check if new remote path exists
        match self
            .sftp_connection()
            .as_ref()
            .unwrap()
            .stat(remote_pathbuf.as_path())
        {
            Ok(stat) => {
                if stat.is_dir() {
                    self.set_remote_cwd(remote_pathbuf.clone());
                    return Ok(remote_pathbuf);
                } else {
                    return Err(format!(
                        "Cannot cd into '{}': not a directory",
                        remote_pathbuf.display()
                    ));
                }
            }
            Err(ssh2_error) => {
                return Err(format!(
                    "Cannot cd into '{}': {}",
                    remote_pathbuf.display(),
                    ssh2_error.to_string()
                ));
            }
        }
    }

    /**
     * tt-bj2: assumes that set_current_dir handles absolute or relative paths
     */
    pub fn cd_local(&mut self, path: &str) -> () {
        std::env::set_current_dir(Path::new(path)).expect("Failed to change local directory");
    }

    pub fn ensure_dir_local(&mut self, path: &Path) -> () {
        let mut path = Path::new(path);
        if path.is_file() {
            path = path.parent().unwrap();
        }

        if !path.exists() {
            std::fs::create_dir_all(path).expect("Failed to create directory");
        }
    }

    /**
     * Get the stats of a remote file or directory
     *
     * Note: Since ssh2 lib has no concept of a remote cd, we have to account for this manually
     * by using self.pwd() as base path for the remote filepath, which accounts for self.remote_cwd
     */
    pub fn stat_remote(&mut self, path: &Path) -> Result<FileStat, ssh2::Error> {
        let remote_pathbuf = self.canonicalize_remote(path);
        return self
            .sftp_connection()
            .as_ref()
            .unwrap()
            .stat(remote_pathbuf.as_path());
    }

    /**
     * Checks if a remote file exists
     *
     * Note: self.stat_remote will account for self.remote_cwd variable
     */
    pub fn has_file_remote(&mut self, path: &Path) -> bool {
        match self.stat_remote(path) {
            Ok(stat) => stat.is_file(),
            Err(_) => false,
        }
    }

    /**
     * Returns true when the given path exists on the remote server and is a directory
     * Errors:
     * - when path is a file
     *
     * Note: self.stat_remote will account for self.remote_cwd variable
     */
    pub fn has_dir_remote(&mut self, path: &Path) -> Result<bool, String> {
        return match self.stat_remote(path) {
            Ok(stat) => {
                if stat.is_file() {
                    Err(String::from("Path is a file, not a directory"))
                } else {
                    // if path is dir, return true (since it exists)
                    Ok(true)
                }
            }
            // if path stats cannot be retrieved, return false since path does not exist
            Err(_) => Ok(false),
        };
    }

    /**
     * Creates a file and it's parent path on the remote server
     *
     * Note: Since ssh2 lib has no concept of a remote cd,
     * we have to account for this manually by using self.pwd()
     * as base path for the remote filepath, which accounts for self.remote_cwd
     */
    pub fn ensure_file_remote(&mut self, path: &Path) -> () {
        let pathbuf = self.canonicalize_remote(path);
        let remote_path = pathbuf.as_path();

        // STEP 1: create the parent directory if it does not exist
        // assume that last component of path is a file
        // cannot verify this, since linux files might not have a file extension
        let parent_path = remote_path.parent().unwrap();
        self.ensure_dir_remote(parent_path);

        // STEP 2: create the file
        let mut file = self
            .sftp_connection()
            .as_ref()
            .unwrap()
            .create(remote_path)
            .expect("Failed to create remote file");
        file.close().expect("Failed to close remote file");
    }

    /**
     * WIP
     */
    pub fn remove_file_remote(&mut self, path: &Path) -> () {
        let pathbuf = self.canonicalize_remote(path);
        let remote_path = pathbuf.as_path();

        self.sftp_connection()
            .as_ref()
            .unwrap()
            .unlink(remote_path)
            .expect("Failed to remove file");
    }

    /**
     * recursive remove
     *
     * CAUTION: This function directly deletes anything at a given path!
     * Make sure to check if the path is correct before calling this function.
     *
     * Example:
     * If path is `playground/subpath`, it will delete the contents of the `subpath` directory and the `subpath` directory itself.
     */
    pub fn rmrf_remote(&mut self, path: &Path) -> () {
        let pathbuf = self.canonicalize_remote(path);
        let remote_path = pathbuf.as_path();

        self.rmrf_remote_recursive(remote_path);
    }

    /**
     * Caution: This function expects a canonicalized remote path!
     */
    fn rmrf_remote_recursive(&mut self, path: &Path) {
        // Step 1: if path is file, delete directly
        if self.has_file_remote(path) {
            self.remove_file_remote(path);
            println!("Removed file: {}", path.display());
            return;
        }

        // path is dir from here on
        // STEP 2: Get all entries in the directory
        let dir_entries = self
            .sftp_connection()
            .as_ref()
            .unwrap()
            .readdir(path)
            .expect("Failed to read directory contents");

        // STEP 3: dir_entries is not empty => delete all entries first
        if !dir_entries.is_empty() {
            for (entry_path, _stat) in dir_entries {
                self.rmrf_remote_recursive(&entry_path);
            }
        }

        // STEP 4: remove empty dir
        self.sftp_connection()
            .as_ref()
            .unwrap()
            .rmdir(path)
            .expect("Failed to remove directory");

        println!("Removed directory: {}", path.display());
    }

    /**
     * Given a remote path, this function ensures that the directory exists
     * Same as mkdir -p (which does not exist in sftp).
     *
     * Current dir should be given as "."
     */
    pub fn ensure_dir_remote(&mut self, path: &Path) -> Result<(), SftpClientError> {
        let mut path_components = path
            .components()
            .map(|c| c.as_os_str().to_str().unwrap())
            .collect::<VecDeque<&str>>();

        // if input path is relative, start ensure dir with the current remote cwd
        // Note: WorkingPath must be a PathBuf here, since it will be pushed to
        let mut working_path = if path.is_relative() {
            self.remote_cwd_as_pathbuf().unwrap()
        } else {
            // path is absolute, set the working_path to the first path_component
            let first_path_comp = path_components.pop_front().map(|c| PathBuf::from(c));

            if let Some(first_path) = first_path_comp {
                first_path
            } else {
                return Err(SftpClientError::RemotePathError {
                    msg: String::from(
                        "Cannot get first path component of remote path - remote path is empty",
                    ),
                    path: path.to_path_buf(),
                });
            }
        };

        while !path_components.is_empty() {
            // .unwrap is safe here, since path_components cannot be empty inside here
            let component = path_components.pop_front().unwrap();
            working_path.push(component);

            // println!("Ensuring directory: {}", working_path.display());

            // check if working_path exists
            let is_dir = match self
                .sftp_connection()
                .as_ref()
                .unwrap()
                .stat(working_path.as_path())
            {
                Ok(stat) => stat.is_dir(),
                Err(_) => false,
            };

            if is_dir {
                continue;
            };

            // create the directory
            let mkdir_result = self
                .sftp_connection()
                .as_ref()
                .unwrap()
                .mkdir(working_path.as_path(), 0o755);

            // check errors on the mkdir
            if let Err(e) = mkdir_result {
                return Err(SftpClientError::RemoteMkdirError {
                    msg: "Failed to create directory".to_string(),
                    path: working_path.clone(),
                    inner_error: e,
                });
            }
        }
        // println!("Created directory: {}", working_path.display());
        Ok(())
    }

    /**
     * Special function for the dev-uploader I write:
     * Ensures a remote dir and caches that this dir exists.
     * If the same dir is ensured again, it will not be testet again on the remote but assumed to exist.
     *
     * CAUTION: This does only work when this dev-uploader is the only programm taht changes the remote dir structure,
     * otherwise the cached state might be wrong.
     *
     * Idea: Each file upload ensures the remote parent dir before uploading.
     * Since most files are in the same dir, this function can be used to ensure the parent dir only once
     * and not incure the network cost of checking the parent dir for each file upload(which is about 248ms, teste on 5G mobile)
     */
    pub fn ensure_dir_remote_cached(&mut self, path: &Path) -> Result<(), SftpClientError> {
        if self.runtime_props.remote_dir_cache.contains_key(path) {
            return Ok(());
        }

        let result = self.ensure_dir_remote(path);
        if result.is_err() {
            return result;
        }

        // If ensure_dir_remtoe returned ok, insert the path into the cache
        let _ = self
            .runtime_props
            .remote_dir_cache
            .insert(path.to_path_buf(), true);
        return Ok(());
    }

    // File Upload functions
    // ----------------------

    /**
     * Add a function to canonicalize a remote path.
     * This is needed so that self.remote_cwd is always taken into account.
     * This function is needed with EVERY SFTP command that needs a remote path.
     */
    fn canonicalize_remote(&mut self, input_path: &Path) -> PathBuf {
        // if input remote path is relative, join it with the current remote cwd
        if input_path.is_relative() {
            self.pwd_remote().join(input_path)
        } else {
            // if input remote path is absolute, return it as is
            PathBuf::from(input_path)
        }
    }

    /**
     * Converts a local path to a remote path (regardless of type file or dir)
     * Uses self.pwd_remote() as remote base path.
     * Uses local cwd as local base path.
     *
     * @param local_path: can be absolute or relative,
     *        absolute path will be converted to relative by stripping the local cwd
     */
    pub fn local_to_remote_path_with_cwds(
        &mut self,
        local_path: &Path,
    ) -> Result<PathBuf, SftpClientError> {
        let local_relative_path =
            compute_relative_path_from_local(local_path, None).map_err(|e| {
                SftpClientError::LocalPathError {
                    msg: format!("Cannot compute local relative path from local: {}", e),
                    path: local_path.to_path_buf(),
                }
            })?;
        return Ok(self.canonicalize_remote(&local_relative_path));
    }

    /**
     * Converts a local path to a remote path (regardless of type file or dir)
     * Uses self.pwd_remote() as remote base path.
     * Uses local_base as local base path.
     *
     * @param local_path: can be absolute or relative,
     *       absolute path will be converted to relative by stripping the local base
     */
    pub fn local_to_remote_path_with_lbase(
        &mut self,
        local_path: &Path,
        local_base: &Path,
    ) -> Result<PathBuf, SftpClientError> {
        let local_relative_path = compute_relative_path_from_local(local_path, Some(local_base))
            .map_err(|e| SftpClientError::LocalPathError {
                msg: format!("Cannot compute local relative path from local: {}", e),
                path: local_path.to_path_buf(),
            })?;
        return Ok(self.canonicalize_remote(&local_relative_path));
    }

    /**
     * Converts a local path to a remote path (regardless of type file or dir)
     * @param local_path: can be absolute or relative,
     *       absolute path will be converted to relative by stripping the local cwd
     * @param remote_base: the remote base path
     *   - can be absolute or relative
     *   - if absolute: remote_path = remote_base + relative_local
     *   - if relative: remote_path = remote_cwd + remote_base + relative_local
     */
    pub fn local_to_remote_path_with_rbase(
        &mut self,
        local_path: &Path,
        remote_base: &Path,
    ) -> Result<PathBuf, SftpClientError> {
        let remote_absolute = self.canonicalize_remote(remote_base);
        let relative_local = compute_relative_path_from_local(local_path, None).map_err(|e| {
            SftpClientError::LocalPathError {
                msg: format!("Cannot compute local relative path from local: {}", e),
                path: local_path.to_path_buf(),
            }
        })?;
        return Ok(remote_absolute.join(relative_local));
    }

    /**
     * Converts a local path to a remote path (regardless of type file or dir)
     *
     * @param local_path: can be absolute or relative,
     *       absolute path will be converted to relative by stripping the local base
     * @param local_base: the local base path
     * @param remote_base: the remote base path
     *   - can be absolute or relative
     *   - if absolute: remote_path = remote_base + relative_local
     *   - if relative: remote_path = remote_cwd + remote_base + relative_local
     *   
     */
    pub fn local_to_remote_path_with_bases(
        &mut self,
        local_path: &Path,
        local_base: &Path,
        remote_base: &Path,
    ) -> Result<PathBuf, SftpClientError> {
        let remote_absolute = self.canonicalize_remote(remote_base);
        let relative_local = compute_relative_path_from_local(local_path, Some(local_base))
            .map_err(|e| SftpClientError::LocalPathError {
                msg: format!("Cannot compute local relative path from local: {}", e),
                path: local_path.to_path_buf(),
            })?;
        Ok(remote_absolute.join(relative_local))
    }

    /**
     * Converts a local path to a remote path (regardless of type file or dir)
     * Decides based on params given, which conversion function to call.
     */
    pub fn local_to_remote_path(
        &mut self,
        local_path: &Path,
        local_base: Option<&Path>,
        remote_base: Option<&Path>,
    ) -> Result<PathBuf, SftpClientError> {
        let remote_path = match (local_base, remote_base) {
            (Some(lbase), Some(rbase)) => {
                self.local_to_remote_path_with_bases(local_path, lbase, rbase)
            }
            (Some(lbase), None) => self.local_to_remote_path_with_lbase(local_path, lbase),
            (None, Some(rbase)) => self.local_to_remote_path_with_rbase(local_path, rbase),
            (None, None) => self.local_to_remote_path_with_cwds(local_path),
        };
        remote_path
    }

    /**
     * Uploads a single file to a precomputed filepath on the remote server.
     * No filepath magic is included in this call (except resolving cwd if input paths are relative),
     * it just takes the explicitely given local filepath and writes it's content to the explicitely given remote filepath.
     * It basically gives complete freedom of the filename and path on the remote server.
     *
     * Example 1: Two Absolute paths  =>
     * - local_filepath: /Users/myuser/mylocalfile.txt
     * - remote_filepath: /opt/myuser/mypath/myfile.md
     *
     * Example 2: Two relative paths =>
     * - local_path: mylocalfile.txt => will be resolved with local cwd to /Users/myuser/mylocalfile.txt
     * - remote_path: mypath/myfile.md => will be resolved with remote cwd to /opt/myuser/mypath/myfile.md
     *
     * Does:
     * - create the parent dir if it does not exist
     * - create the file if it does not exist
     * - overwrites the file contents if it exists
     *
     * Special Extension:
     * If allow_cached_ensure_remote_dir is true, use self.ensure_dir_remote_cached(),
     * which ensures each unique parent dir path only once.
     * CAUTION: Does break when the remote dir structure is changed by another process while uploading!
     *
     *
     */
    pub fn upload_file_explicit(
        &mut self,
        local_filepath: &Path,
        remote_filepath: &Path,
        allow_cached_ensure_remote_dir: bool,
    ) -> Result<(), SftpClientError> {
        // STEP 0: prepare vars
        const BUFFER_SIZE: usize = 128 * 1024; // 128KB

        // Step 1: prepare local path
        let local_pathbuf =
            local_filepath
                .canonicalize()
                .map_err(|e| SftpClientError::LocalPathError {
                    msg: format!("Cannot canonicalize local path: {}", e),
                    path: local_filepath.to_path_buf(),
                })?;
        let local_path = local_pathbuf.as_path();

        // Step 2: prepare remote path
        let remote_pathbuf = self.canonicalize_remote(remote_filepath);
        let remote_path = remote_pathbuf.as_path();

        // STEP 2: open the local file for reading
        let src_file =
            std::fs::File::open(local_path).map_err(|e| SftpClientError::OpenLocalFileError {
                path: remote_path.to_path_buf(),
                io_error: e,
            })?;
        let mut reader = BufReader::with_capacity(BUFFER_SIZE, src_file);

        // STEP 3: ensure the remote parent directory exists
        let remote_dir = match remote_path.parent() {
            Some(parent) => parent,
            None => {
                return Err(SftpClientError::RemotePathError {
                    msg: String::from("Cannot get parent directory of remote path"),
                    path: remote_path.to_path_buf(),
                });
            }
        };
        if allow_cached_ensure_remote_dir {
            self.ensure_dir_remote_cached(remote_dir)?;
        } else {
            self.ensure_dir_remote(remote_dir)?;
        }

        // STEP 4.1: get sftp connection, returns error if not connected
        let sftp = self.sftp_connection()?;

        // STEP 4.2: open the file and write the contents
        let mut remote_file = sftp
            .open_mode(
                remote_path,
                // if read is needed: add ssh2::OpenFlags::READ
                ssh2::OpenFlags::CREATE | ssh2::OpenFlags::WRITE | ssh2::OpenFlags::TRUNCATE,
                // file permissions when creating a file
                0o644,
                ssh2::OpenType::File,
            )
            .map_err(|e| SftpClientError::OpenRemoteFileError {
                path: remote_path.to_path_buf(),
                ssh2_error: e,
            })?;

        // 128KB buffer
        let mut writer = BufWriter::with_capacity(BUFFER_SIZE, &mut remote_file);

        // STEP 5: copy the contents from the local file to the remote file with std::io::copy
        match copy(&mut reader, &mut writer) {
            Ok(_) => {}
            Err(e) => {
                return Err(SftpClientError::LocalToRemoteCopyError {
                    local_path: local_filepath.to_path_buf(),
                    remote_path: remote_path.to_path_buf(),
                    io_error: e,
                });
            }
        };

        // STEP 6: flush the writer (in case something was not written yet)
        writer
            .flush()
            .map_err(|e| SftpClientError::LocalToRemoteCopyError {
                local_path: local_filepath.to_path_buf(),
                remote_path: remote_path.to_path_buf(),
                io_error: e,
            })?;

        drop(writer); // This will close the compat wrapper

        // STEP 7: close the remote file
        remote_file
            .close()
            .map_err(|e| SftpClientError::CloseRemoteFileError {
                path: remote_path.to_path_buf(),
                ssh2_error: e,
            })?;

        // reader will auto-close when it goes out of scope
        Ok(())
    }

    /**
     * Convenience function to sync a file to a remote dir, providing options for
     * remote_path_resolution
     *
     * @param local_filepath: the local filepath to sync (respecting the local_base param)
     * @param remote_dir: the remote dir to sync to (can be either "absolute" or "relative to remote cwd")
     * @param local_base: the local base dir to resolve the local_filepath against (uses local_cwd if not provided)
     * @param allow_cached_ensure_remote_dir: if true, use self.ensure_dir_remote_cached(),
     * which ensures each unique parent dir path only once.
     * CAUTION: Does break when the remote dir structure is changed by another process while uploading!
     */
    pub fn sync_file_to_dir(
        &mut self,
        local_filepath: &Path,
        remote_dir: &Path,
        local_base: Option<&Path>,
        allow_cached_ensure_remote_dir: bool,
    ) -> Result<(), SftpClientError> {
        let remote_pathbuf_result = match local_base {
            Some(local_base) => {
                self.local_to_remote_path_with_bases(local_filepath, local_base, remote_dir)
            }
            None => self.local_to_remote_path_with_rbase(local_filepath, remote_dir),
        };
        let remote_pathbuf = remote_pathbuf_result?;
        self.upload_file_explicit(
            local_filepath,
            remote_pathbuf.as_path(),
            allow_cached_ensure_remote_dir,
        )
    }

    /**
     * Syncs a file to the remote cwd, using local cwd or a specified local base dir
     */
    pub fn sync_file_to_cwd(
        &mut self,
        local_filepath: &Path,
        local_base: Option<&Path>,
        allow_cached_ensure_remote_dir: bool,
    ) -> Result<(), SftpClientError> {
        let remote_pathbuf_result = match local_base {
            Some(local_base) => self.local_to_remote_path_with_lbase(local_filepath, local_base),
            None => self.local_to_remote_path_with_cwds(local_filepath),
        };
        let remote_pathbuf = remote_pathbuf_result?;
        self.upload_file_explicit(
            local_filepath,
            remote_pathbuf.as_path(),
            allow_cached_ensure_remote_dir,
        )
    }
}

#[cfg(test)]
mod tests {
    use core::{assert_eq, fmt};
    use insta::assert_debug_snapshot;
    use once_cell::sync::Lazy;
    use std::{io::Write, sync::Mutex};

    use super::*;

    // Test Helper Functions
    // ---------------------

    fn init_sftp_client() -> SftpClient {
        let mut client = SftpClient::with_password(
            "dev_uploader - Unit Test",
            "localhost",
            2022,
            "test",
            "test",
        );

        client.connect();
        return client;
    }

    // -------------------
    // Shared test fixture
    // -------------------
    struct TestFixture {
        client: SftpClient,
        initial_cwd: PathBuf,
    }

    impl TestFixture {
        fn new() -> Self {
            let cwd = std::env::current_dir().expect("Could not read current working directory");
            println!("INITIAL_CWD: {:?}", &cwd);

            TestFixture {
                client: init_sftp_client(),
                initial_cwd: cwd,
            }
        }
    }

    // Static fixture accessible by all tests
    // this allow(dead_code) is needed because the rust linter does not detect the usage in the tests. It only considers usages in the main programm a "usage"
    #[allow(dead_code)]
    static TEST_FIXTURE: Lazy<Mutex<TestFixture>> = Lazy::new(|| Mutex::new(TestFixture::new()));

    // Test Functions
    // --------------

    // TODO: This test runs through but the file does not appear in the mounted volume
    #[test]
    fn test_sftp_open_file_raw() {
        let mut fixture = TEST_FIXTURE.lock().unwrap();
        let client = &mut fixture.client;

        // let file_path = Path::new("relative_dir/test.txt");
        // let file_path = Path::new("subpath/test.txt");
        let cwd_remote = client.pwd_remote();
        let file_path = cwd_remote.join("open_file_raw.txt");

        assert!(client
            .ensure_dir_remote(file_path.parent().unwrap())
            .is_ok());

        let mut test_file = match client.sftp_connection().as_ref().unwrap().open_mode(
            file_path.as_path(),
            // if read is needed: add ssh2::OpenFlags::READ
            ssh2::OpenFlags::CREATE | ssh2::OpenFlags::WRITE | ssh2::OpenFlags::TRUNCATE,
            // file permissions when creating a file
            0o644,
            ssh2::OpenType::File,
        ) {
            Ok(file) => file,
            Err(e) => panic!("Failed to open file: {}", e),
        };

        // Assert that the file is available
        assert!(client.has_file_remote(file_path.as_path()));

        test_file
            .write_fmt(format_args!("hello {}", "world"))
            .expect("Failed to write to file!");

        test_file.close().expect("Failed to close file!");

        // CLEANUP
        client.rmrf_remote(&file_path);
        assert!(client.has_file_remote(file_path.as_path()) == false);
    }
}
