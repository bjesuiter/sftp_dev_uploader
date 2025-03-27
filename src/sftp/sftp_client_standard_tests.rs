use crate::sftp::sftp_client::SftpClient;
use insta::assert_debug_snapshot;
use once_cell::sync::Lazy;
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

/**
 * TODOs:
 * - test pubkey based auth!
 */

// ---------------------
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

// ----------
// TEST CASES
// ----------

#[test]
fn test_init_sftp_client_fixture() {
    let mut fixture = TEST_FIXTURE.lock().unwrap();
    let client = &mut fixture.client;

    assert!(client.uploader_name.starts_with("dev_uploader"));
    assert!(fixture.initial_cwd.starts_with("/"));
}

// CAUTION: This test does only work when connecting to a real SSH server! The sftpgo test environment does not support this.
// #[test]
// fn test_exec_ssh2_command() {
//     let mut fixture = TEST_FIXTURE.lock().unwrap();
//     let client = &mut fixture.client;

//     let output = client.exec_ssh_command("whoami");
//     assert_eq!(output.trim(), "test");

//     // channel.wait_close().unwrap();
//     // println!("Exit status: {}", channel.exit_status().unwrap());
// }

#[test]
fn test_sftp_pwd_with_validation() {
    let mut fixture = TEST_FIXTURE.lock().unwrap();
    let client = &mut fixture.client;

    let out = client.pwd_remote_with_validation();
    assert_eq!(out.is_ok(), true);

    client.set_remote_cwd(PathBuf::from("/non_existing_dir"));
    let out2 = client.pwd_remote_with_validation();
    assert_eq!(out2.is_err(), true);

    // CLEANUP
    let initial_cwd = client.initial_pwd_remote();
    client.set_remote_cwd(initial_cwd);
}

#[test]
fn test_sftp_ls() {
    let mut fixture = TEST_FIXTURE.lock().unwrap();
    let client = &mut fixture.client;

    let out = client.ls_remote(None).expect("ls failed on remote server");
    let path_vec = out
        .iter()
        .map(|path_buf| {
            return path_buf.as_path();
        })
        .collect::<Vec<&Path>>();

    assert_debug_snapshot!(path_vec);
}

#[test]
fn test_sftp_lls() {
    let mut fixture = TEST_FIXTURE.lock().unwrap();
    let client = &mut fixture.client;

    client.cd_local("testfiles");

    let out = client.ls_local(None);
    let local_path_vec = out
        .iter()
        .map(|path_buf| {
            return path_buf.as_path();
        })
        .collect::<Vec<&Path>>();

    assert_debug_snapshot!(local_path_vec);

    // CLEANUP
    client.cd_local("..");
}

#[test]
fn test_sftp_cd() {
    let mut fixture = TEST_FIXTURE.lock().unwrap();
    let client = &mut fixture.client;

    // Test 1: cd into existing directory
    let pwd1 = client.pwd_remote().to_path_buf();
    client
        .cd_remote("initial_testdir")
        .expect("Failed to cd into initial_testdir");
    let pwd2 = client.pwd_remote().to_path_buf();
    assert_eq!(pwd2, pwd1.join("initial_testdir"));

    // Test 2: cd into non-existing directory
    let result = client.cd_remote("non_existing_dir");
    assert_eq!(result.is_err(), true);

    // Test 3: cd back to parent directory
    let last_cwd = client
        .cd_remote("..")
        .expect("Failed to cd back to parent directory after test");

    assert_eq!(last_cwd, pwd1);
}

#[test]
fn test_sftp_lcd() {
    let mut fixture = TEST_FIXTURE.lock().unwrap();
    let client = &mut fixture.client;

    let pwd1 = client
        .pwd_local()
        .expect("Failed to get local current working directory");
    client.cd_local("testfiles");
    let pwd2 = client
        .pwd_local()
        .expect("Failed to get local current working directory");

    assert_eq!(pwd2.as_path(), pwd1.join("testfiles").as_path());

    // Cleanup
    client.cd_local("..");
}

#[test]
fn test_sftp_local_to_remote_path() {
    let mut fixture = TEST_FIXTURE.lock().unwrap();
    let client = &mut fixture.client;

    let watch_folder_relative = Path::new("testfiles");
    let watch_folder_absolute = std::env::current_dir().unwrap().join(watch_folder_relative);

    // check that i can compute the absolute path from the relative path with the current working directory as base path
    assert_eq!(
        watch_folder_absolute,
        watch_folder_relative.canonicalize().unwrap()
    );

    // check that i can compute the relative path from the absolute path with the current working directory as base path
    assert_eq!(
        watch_folder_relative,
        watch_folder_absolute
            .strip_prefix(std::env::current_dir().unwrap())
            .unwrap()
    );

    // Assert 1 -  assume "/"" as remote cwd
    assert_eq!(client.pwd_remote().to_path_buf(), PathBuf::from("/"));

    // Assert 2 - calculate the remote path for testfile1 by using the local cwd and the remote cwd as base paths.
    let testfile1 = Path::new("testfiles/depth1/depth2/testfile-depth2.txt")
        .canonicalize()
        .unwrap();
    let testfile1_relative_to_cwd = client.local_to_remote_path(&testfile1, None, None).unwrap();

    assert_eq!(
        testfile1_relative_to_cwd,
        PathBuf::from("/testfiles/depth1/depth2/testfile-depth2.txt")
    );

    // Assert 3 - calculate the remote path for testfile1 by using the local watchdir and the remote cwd as base paths.
    let testfile1_relative_to_watchdir = client
        .local_to_remote_path(testfile1.as_path(), Some(Path::new("testfiles")), None)
        .unwrap();

    assert_eq!(
        testfile1_relative_to_watchdir,
        PathBuf::from("/depth1/depth2/testfile-depth2.txt")
    );
}

/**
 * This test uploads a file without changing local cwd or remote cwd.
 * It simply uploads a file from an explicit local path to an explicit remote path.
 */
#[test]
fn test_sftp_upload_file_explicit_remote() {
    let mut fixture = TEST_FIXTURE.lock().unwrap();
    let client = &mut fixture.client;

    // Assert 1 - assume "/"" as remote cwd
    assert_eq!(client.pwd_remote().to_path_buf(), PathBuf::from("/"));

    // Assert 2 - assume that /explicit_remote_dir does not exist
    assert_eq!(
        client
            .has_dir_remote(Path::new("/explicit_remote_dir"))
            .unwrap_or(false),
        false
    );

    let local_path = Path::new("testfiles/upload_file_explicit_remote.md");
    let remote_path = Path::new("explicit_remote_dir/upload_file_explicit_remote.txt");

    client
        .upload_file_explicit(local_path, remote_path, false)
        .expect("Failed to upload file");

    assert!(client.has_file_remote(remote_path));

    // Cleanup
    client.rmrf_remote(Path::new("/explicit_remote_dir"));
}

#[test]
fn test_sftp_upload_file_implicit_remote() {
    let mut fixture = TEST_FIXTURE.lock().unwrap();
    let client = &mut fixture.client;

    // Assert 1 - assume "/"" as remote cwd
    assert_eq!(client.pwd_remote().to_path_buf(), PathBuf::from("/"));

    // Assert 2 - assume that /testfiles does not exist
    assert_eq!(
        client
            .has_dir_remote(Path::new("/testfiles"))
            .unwrap_or(false),
        false
    );

    let local_path = Path::new("testfiles/upload_file_implicit_remote.md");
    client
        .sync_file_to_cwd(local_path.canonicalize().unwrap().as_path(), None, false)
        .expect("Failed to upload file");

    let remote_path = Path::new("testfiles/upload_file_implicit_remote.md");
    assert!(client.has_file_remote(remote_path));

    // CLEANUP
    let parent_dir = remote_path.parent().unwrap();
    client.rmrf_remote(parent_dir);
}

#[test]
fn test_sftp_upload_file_implicit_different_cwds_remote() {
    let mut fixture = TEST_FIXTURE.lock().unwrap();
    let client = &mut fixture.client;

    // copy file from local dir "testfiles" to remote dir "playground", but keep the relative path constant
    client.cd_local("testfiles");
    let new_remote_cwd = Path::new("explicit_remote_cwd");
    client
        .ensure_dir_remote(new_remote_cwd)
        .expect("Failed to ensure remote dir");
    client
        .cd_remote(new_remote_cwd.to_str().unwrap())
        .expect("Failed to cd into 'explicit_remote_cwd'");

    // Note: the relative_dir is a subfolder of "testfiles" on the local machine, since the local cwd is reset to "testfiles" above
    let relative_path = Path::new("relative_dir/upload_file_implicit_remote.md");
    let absolute_path = relative_path.canonicalize().unwrap();

    // The logic:
    // - local_base is None, since the relative_dir is a subfolder of "testfiles" on the local machine, and "testfiles" is the current cwd on the local machine
    // - allow_cached_ensure_remote_dir is false, since we do not want to use the dir cache, if the remote dir exist
    // - remote_dir is "explicit_remote_cwd", since we want to upload the file to the remote dir "explicit_remote_cwd", but keep the relative path constant
    client
        .sync_file_to_dir(absolute_path.as_path(), Path::new("."), None, false)
        .expect("Failed to upload file");

    assert!(client.has_file_remote(relative_path));

    // CLEANUP
    client.cd_local("..");
    client
        .cd_remote("..")
        .expect("Failed to cd back to parent directory");
    client.rmrf_remote(new_remote_cwd);
}

#[test]
fn test_sftp_local_ensure_dir() {
    let mut fixture = TEST_FIXTURE.lock().unwrap();
    let client = &mut fixture.client;

    let dir_path = Path::new("testfiles/depth1.1/depth2.2");
    let has_dir_before = match std::fs::metadata(dir_path) {
        Ok(metadata) => metadata.is_dir(),
        Err(_) => false,
    };
    assert!(has_dir_before == false);

    client.ensure_dir_local(dir_path);

    let has_dir_after = match std::fs::metadata(dir_path) {
        Ok(metadata) => metadata.is_dir(),
        Err(_) => false,
    };
    assert!(has_dir_after);

    // Remove the directory if it exists to clean up for next tests
    let remove_path = Path::new("testfiles/depth1.1");
    if std::fs::metadata(remove_path).is_ok() {
        std::fs::remove_dir_all(remove_path).expect("Failed to remove directory");
    }
}

#[test]
fn test_sftp_ensure_and_remove_file_remote() {
    let mut fixture = TEST_FIXTURE.lock().unwrap();
    let client = &mut fixture.client;

    let file_path = Path::new("testfiles/depth1/depth2/ensured_remote_file.txt");

    // STEP 1: create the file
    client.ensure_file_remote(file_path);

    // Assert 1: check if the file exists
    assert!(client.has_file_remote(file_path));

    // STEP 2: remove the file
    client.remove_file_remote(file_path);

    // Assert 2: check if the file is removed
    assert!(client.has_file_remote(file_path) == false);

    // STEP 3: check if the parent dir of the file is still there
    match client.has_dir_remote(file_path.parent().unwrap()) {
        Ok(has_dir) => assert!(has_dir),
        Err(_) => assert!(false),
    };

    // CLEANUP: remove full path after test
    let remove_path = Path::new("testfiles");
    client.rmrf_remote(remove_path);

    // Assert 3: check if the full path is removed
    assert!(client.has_dir_remote(remove_path).unwrap_or(false) == false);
}

#[test]
fn test_sftp_ensure_and_remove_dir_remote_absolute() {
    let mut fixture = TEST_FIXTURE.lock().unwrap();
    let client = &mut fixture.client;

    let cwd = client.pwd_remote();
    let dir1_path = cwd.join("ensure_and_remove_dir_remote_absolute/depth1");
    let dir2_path = cwd.join("ensure_and_remove_dir_remote_absolute/depth1/depth2");

    // Assert 1: check if dirs do not exist
    assert!(client.has_dir_remote(dir1_path.as_path()).unwrap_or(false) == false);
    assert!(client.has_dir_remote(dir2_path.as_path()).unwrap_or(false) == false);

    // TEST part 1: ensure dirs
    assert!(client.ensure_dir_remote(dir2_path.as_path()).is_ok());

    // Assert 2: check if dirs exist
    assert!(client.has_dir_remote(dir1_path.as_path()).unwrap_or(false));
    assert!(client.has_dir_remote(dir2_path.as_path()).unwrap_or(false));

    // TEST part 2: remove dirs
    let parent_dir = dir1_path.parent().unwrap();
    client.rmrf_remote(parent_dir);

    // Assert 3: check if dirs are removed
    assert!(client.has_dir_remote(parent_dir).unwrap_or(false) == false);
}

#[test]
fn test_sftp_ensure_and_remove_dir_remote_relative() {
    let mut fixture = TEST_FIXTURE.lock().unwrap();
    let client = &mut fixture.client;

    let dir1_path = PathBuf::from("ensure_and_remove_dir_remote_relative/depth1");
    let dir2_path = PathBuf::from("ensure_and_remove_dir_remote_relative/depth1/depth2");

    // Assert 1: check if dirs do not exist
    assert!(client.has_dir_remote(dir1_path.as_path()).unwrap_or(false) == false);
    assert!(client.has_dir_remote(dir2_path.as_path()).unwrap_or(false) == false);

    // TEST part 1: ensure dirs
    assert!(client.ensure_dir_remote(dir2_path.as_path()).is_ok());

    // Assert 2: check if dirs exist
    assert!(client.has_dir_remote(dir1_path.as_path()).unwrap_or(false));
    assert!(client.has_dir_remote(dir2_path.as_path()).unwrap_or(false));

    // TEST part 2: remove dirs
    let parent_dir = dir1_path.parent().unwrap();
    client.rmrf_remote(parent_dir);

    // Assert 3: check if dirs are removed
    assert!(client.has_dir_remote(parent_dir).unwrap_or(false) == false);
}
