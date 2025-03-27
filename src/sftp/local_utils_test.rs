use std::{io::ErrorKind, path::Path};

use super::local_utils::compute_relative_path_from_local;

#[test]
fn test_compute_relative_path_from_local() {
    // TEST1: error if base_path does not exist locally
    let invalid_input_path = Path::new("/home/myuser/mypath/myfile.txt");
    let invalid_base_path = Path::new("/home/myuser");
    let result1 = compute_relative_path_from_local(invalid_input_path, Some(invalid_base_path));
    assert!(result1.is_err());
    if let Err(e) = result1 {
        // assert that base_path could not found
        assert_eq!(e.kind(), ErrorKind::NotFound);
    }

    // TEST2: success if base_path exists locally
    let input_path = Path::new("/Users/bjesuiter/mypath/myfile.txt");
    let valid_base_path = Path::new("/Users/bjesuiter");
    let result2 = compute_relative_path_from_local(input_path, Some(valid_base_path));
    assert_eq!(result2.is_ok(), true);
    assert_eq!(result2.unwrap().to_str().unwrap(), "mypath/myfile.txt");

    // TEST3: error if input_path is relative
    let input_path = Path::new("mypath/myfile.txt");
    let result3 = compute_relative_path_from_local(input_path, None);
    assert!(result3.is_err());
    if let Err(e) = result3 {
        // assert that input_path must be absolute
        assert_eq!(e.kind(), ErrorKind::InvalidInput);
    }
}
