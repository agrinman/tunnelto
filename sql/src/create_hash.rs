use sha2::Digest;
use std::env;

fn key_id(auth_key: &str) -> String {
    let hash = sha2::Sha256::digest(auth_key.as_bytes()).to_vec();
    base64::encode_config(&hash, base64::URL_SAFE_NO_PAD)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Please provide a key to hash!");
    }
    else {
        println!("{}", key_id(&args[1]));
    }
}
