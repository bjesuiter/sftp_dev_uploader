use cli::setup_cli;
use cli::upload_pair::UploadPair;
use std::path::PathBuf;
use uploader::upload_actor::AuthMethod;
use uploader::upload_actor_handle::UploadActorHandle;
use watcher::watch_actor_handle::start_watching;

mod cli;
mod sftp;
mod uploader;
mod utils;
mod watcher;

fn main() {
    let cli = setup_cli();
    let matches = cli.get_matches();

    // upload_pair is required, so unwrap is safe
    let upload_pair =
        UploadPair::from_uploadpair_string(matches.get_one::<String>("upload_pair").unwrap());
    println!("upload_pair: {:?}", upload_pair);

    // connection_count has a default value, so unwrap is safe
    let connection_count = matches.get_one::<u8>("connection_count").unwrap();
    println!("connection_count: {:?}", connection_count);

    // host is required, so unwrap is safe
    let sftp_host = matches.get_one::<String>("host").unwrap();
    println!("sftp_host: {:?}", sftp_host);

    // port is required, so unwrap is safe
    let sftp_port = matches.get_one::<u16>("port").unwrap();
    println!("sftp_port: {:?}", sftp_port);

    let ignore_includes = matches
        .get_many::<String>("watcher_ignore_path_includes")
        .unwrap_or_default()
        .map(|v| v.as_str())
        .collect::<Vec<_>>();
    println!("ignore_includes: {:?}", ignore_includes);

    let ignore_ends = matches
        .get_many::<String>("watcher_ignore_path_ends_with")
        .unwrap_or_default()
        .map(|v| v.as_str())
        .collect::<Vec<_>>();
    println!("ignore_ends: {:?}", ignore_ends);

    // username is required, so unwrap is safe
    let sftp_username = matches.get_one::<String>("username").unwrap();
    println!("sftp_username: {:?}", sftp_username);

    // Read all possible auth inputs
    let pubkey = matches.get_one::<PathBuf>("pubkey");
    println!("pubkey: {:?}", pubkey);

    let privkey = matches.get_one::<PathBuf>("privkey");
    println!("privkey: {:?}", privkey);

    let passphrase = matches.get_one::<String>("passphrase");
    println!("passphrase: {:?}", "******");

    let password = matches.get_one::<String>("password");
    println!("password: {:?}", "******");

    // check if any auth method is provided
    if password.is_none() && (pubkey.is_none() || privkey.is_none()) {
        panic!("Either password or pubkey and privkey must be provided!");
    }

    // Read extra flags
    let upload_initial = matches.get_one::<bool>("upload_initial").unwrap();
    println!("upload_initial: {:?}", upload_initial);

    // Setp 1: Setup watcher thread
    let rx_files_to_upload = match start_watching(
        upload_pair.source.clone(),
        upload_initial.clone(),
        ignore_includes
            .into_iter()
            .map(|s| String::from(s))
            .collect(),
        ignore_ends.into_iter().map(|s| String::from(s)).collect(),
    ) {
        Ok(rx) => rx,
        Err(e) => panic!("Error watching directory: {:?}", e),
    };

    // Step 2: Setup uploader thread
    let auth_method = match pubkey.as_ref() {
        Some(p) => AuthMethod::Pubkey(
            p.to_path_buf(),
            privkey.as_ref().unwrap().to_path_buf(),
            None,
        ),
        None => AuthMethod::Password(password.as_ref().unwrap().to_string()),
    };

    let mut uploader_handle = UploadActorHandle::new(
        connection_count.clone(),
        sftp_host.to_string(),
        sftp_port.clone(),
        sftp_username.to_string(),
        auth_method,
    );

    // Step 3: Start the main loop and send files from watcher to uploader
    while let Ok(files_to_upload) = rx_files_to_upload.recv() {
        // println!(
        //     "Debug: Files received from watcher channel: {:?}",
        //     files_to_upload.len()
        // );
        let remote_dir = Some(upload_pair.target.clone());
        if let Err(e) = uploader_handle.upload_files(
            files_to_upload,
            remote_dir,
            Some(upload_pair.source.clone()),
        ) {
            eprintln!(
                "Error sending files for uploading to the upload actor: {:?}",
                e.to_string()
            );
        }
    }
}
