# sftp_dev_uploader

## Docs

- clap (cli args parser): https://docs.rs/clap/latest/clap/
- watchexec (file watcher): https://docs.rs/watchexec/latest/watchexec/
- Rust by example (General Rust knowledge): https://doc.rust-lang.org/rust-by-example/index.html

## Setup (for development)

- Install cargo-insta (for Snapshot Testing) extension for cargo cli: `cargo install cargo-insta`
- Run `docker compose up -d` to start the sftpgo docker container for the first time (only needed once for user setup)
- Goto `http://localhost:8080` to access the SFTP Go web-ui and make the basic config (it will be persisted to the ./docker/volumes folder in this repo, which is ignored by git)
  - Configure the admin user as user `admin` with password `admin` (since it is only used for testing locally), do not configure 2FA
  - In the left menu, go to `Users`
    - add the user `playground` with password `playground`
    - add the user `test` with password `test`
    - optional (not implemented yet): add the user `test_pubkey` with password `test_pubkey` and a public key of your choice, to test public key auth
      Note that all standard tests (`bx test`) use the username+password login for simplicity. To test public key auth, run `bx testsuite_advanced` (not implemented yet)

## Install Dependencies

Simply build the project with, for example `bx build-debug`, to install all dependencies.

## Release a new version (manual)

- Update version in Cargo.toml
- Run `bx test` to run all tests
- Run `bx build-release` to build the release binary
- Find the release binary in `target/release/dev_uploader`

## Release a new version (automated via Github actions)

Prepare

- Update version in Cargo.toml
- Update the changelog.md

Testing

- Start Docker Desktop / Orbstack, if not running
- Run `docker compose up -d` to start the sftpgo container for testing
- Run `bx test` to run all tests

Push & Release

- Commit the new version in Cargo.toml and create a new tag with the same version (e.g. v1.2.0)
- Push the tag => github actions will build the new release
- Go to the release draft: https://github.com/bjesuiter/sftp_dev_uploader/releases
- add the changelog and publish it

## More useful scripts

- Run `bx test -- <test-name>` to run a specific test (giving it the test name or the file with all the tests as argument)
- Run `bx test -- --nocapture <test-name>` to run a specific test with stdout output
- Run `bx test -- --nocapture <test-name>` to run a specific test with stdout output
- Run `bx test -- --nocapture <test-name>` to run a specific test with stdout output
- Run `bx test -- --nocapture <test-name>` to run a specific test with stdout output

---

# Optimization Ideas

## Test with old dev-uploader:

- 92 Files, 6 Threads, max 15 sek
- rust: 10 Thread, 206 Files, etwa 2:30 min

## Profiling with Samply

cargo install --locked samply

## Idea 1 - manual writes with pipelining

For optimizing writes to a remote SSH2::File, here's the plan:

1. Pipeline writes to reduce latency impact
2. Use vectored writes where possible
3. Optimize buffer management

```rust
use std::io::{BufReader, Write};
use ssh2::File as SshFile;

const BUFFER_SIZE: usize = 128 * 1024; // 128KB
const PIPELINE_CHUNKS: usize = 4;  // Number of chunks to pipeline

pub fn optimized_remote_copy(
    mut reader: BufReader<std::fs::File>,
    remote_file: &mut SshFile,
) -> std::io::Result<u64> {
    let mut buffers = vec![vec![0u8; BUFFER_SIZE]; PIPELINE_CHUNKS];
    let mut total_written = 0u64;

    loop {
        let mut pending_writes = Vec::with_capacity(PIPELINE_CHUNKS);

        // Fill multiple buffers
        for buffer in &mut buffers {
            match reader.read(buffer) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    pending_writes.push(&buffer[..n]);
                }
                Err(e) => return Err(e),
            }
        }

        if pending_writes.is_empty() {
            break;
        }

        // Write all pending buffers in one call if possible
        if remote_file.write_vectored(&pending_writes).is_ok() {
            total_written += pending_writes.iter().map(|b| b.len() as u64).sum::<u64>();
        } else {
            // Fallback to individual writes if vectored write fails
            for chunk in pending_writes {
                remote_file.write_all(chunk)?;
                total_written += chunk.len() as u64;
            }
        }
    }

    remote_file.flush()?;
    Ok(total_written)
}
```

Key optimizations:

- Pipeline multiple chunks to reduce latency impact
- Use vectored writes when supported
- Pre-allocate fixed buffers to reduce memory churn
- Keep existing thread-per-file approach
- Maintain 128KB buffer size since it's already optimal

Note: Actual performance gains depend on network latency and SSH server capabilities. Test with your specific use case.

---

# Dev Docs

## Setup for Cross-Compilation (not working without linter support!)

This only installs a respective rust std for the target platforms but does not provide the full build environment.

Add more rust targets via:

- List all targets: `rustup target list`
- Add target: `rustup target add <target>`

## Verify Static and Dynamic Linking with otool on macos

```bash
    otool -L ./target/release/dev_uploader

    # Example output from a github actions build (before linking ssh2 statically)
    ~/Downloads took 7s
    ✖ otool -L dev_uploader
    dev_uploader:
        /System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation (compatibility version 150.0.0, current version 2503.1.0)
        /System/Library/Frameworks/CoreServices.framework/Versions/A/CoreServices (compatibility version 1.0.0, current version 1226.0.0)
        /opt/homebrew/opt/openssl@3/lib/libssl.3.dylib (compatibility version 3.0.0, current version 3.0.0)
        /opt/homebrew/opt/openssl@3/lib/libcrypto.3.dylib (compatibility version 3.0.0, current version 3.0.0)
        /usr/lib/libz.1.dylib (compatibility version 1.0.0, current version 1.2.12)
        /usr/lib/libiconv.2.dylib (compatibility version 7.0.0, current version 7.0.0)
        /usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1345.120.2)

    # Example output after linking ssh2 statically (from a local build)
    bx see-linked-libs-prod
    target/release/dev_uploader:
        /System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation (compatibility version 150.0.0, current version 3208.0.0)
        /System/Library/Frameworks/CoreServices.framework/Versions/A/CoreServices (compatibility version 1.0.0, current version 1226.0.0)
        /usr/lib/libz.1.dylib (compatibility version 1.0.0, current version 1.2.12)
        /usr/lib/libiconv.2.dylib (compatibility version 7.0.0, current version 7.0.0)
        /usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1351.0.0)
```

---

# Changelog

## Release 1.1.4 - 2025-03-31

- fix github actions release workflow

## Release 1.1.2 & 1.1.3 - 2025-03-31

- finish github actions release workflow

## Release 1.1.1 - 2025-03-27

- fix mismatch between tag and build version in Cargo.toml

## Release 1.1.0 - 2025-03-27

- first public release
- add passphrase support for public key auth
- add support for password based sftp auth

## Release 1.0.2 - 2024-12-02

- rename binary to dev_uploader

## Release 1.0.1 - 2024-12-02

- Re-integrate precreation of remote dirs due to errors when two or more worker threads try to create the same dir in parallel

## Release 1.0.0 - 2024-12-02

- Initial Release
