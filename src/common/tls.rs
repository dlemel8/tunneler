use std::error::Error;
use std::path::PathBuf;

use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio_rustls::rustls::sign::{RSASigningKey, SigningKey};
use tokio_rustls::rustls::{Certificate, PrivateKey};

pub async fn load_certificate(der_path: PathBuf) -> Result<Certificate, Box<dyn Error>> {
    let content = load_file_content(der_path).await?;
    Ok(Certificate(content))
}

pub async fn load_private_key(der_path: PathBuf) -> Result<PrivateKey, Box<dyn Error>> {
    let content = load_file_content(der_path).await?;
    Ok(PrivateKey(content))
}

pub async fn load_signing_key(der_path: PathBuf) -> Result<Box<dyn SigningKey>, Box<dyn Error>> {
    let private_key = load_private_key(der_path).await?;
    match RSASigningKey::new(&private_key) {
        Ok(s) => Ok(Box::new(s)),
        Err(_) => Err("failed to convert private key to signing key".into()),
    }
}

async fn load_file_content(path: PathBuf) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut file = File::open(path).await?;
    let mut content = vec![];
    file.read_to_end(&mut content).await?;
    Ok(content)
}
