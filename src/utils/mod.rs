pub fn split_to_n_chunks<T: Clone>(array: Vec<T>, n: usize) -> Vec<Vec<T>> {
    if n == 0 {
        panic!("n must be greater than 0");
    }

    let mut input = array.clone();
    let mut result = Vec::new();
    let mut i = n;

    while i > 0 {
        let chunk_size = (input.len() as f64 / i as f64).ceil() as usize;
        let chunk: Vec<T> = input.drain(0..chunk_size.min(input.len())).collect();
        result.push(chunk);
        i -= 1;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_to_n_chunks_exact() {
        let array = vec![1, 2, 3, 4, 5, 6, 7, 8, 9];
        let chunks = split_to_n_chunks(array, 3);
        assert!(chunks.len() == 3);
        println!("{:?}", chunks);
    }

    #[test]
    fn test_split_to_n_chunks_overflow() {
        let array = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let chunks = split_to_n_chunks(array, 3);
        assert!(chunks.len() == 3);
        println!("{:?}", chunks);
    }

    #[test]
    fn test_split_to_n_chunks_underflow() {
        let array = vec![1, 2, 3, 4];
        let chunks = split_to_n_chunks(array, 6);
        assert!(chunks.len() == 6);
        println!("{:?}", chunks);
    }
}
