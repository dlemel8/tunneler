use std::error::Error;
use std::path::PathBuf;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio_rustls::rustls::{Certificate, PrivateKey};

pub async fn load_certificate(der_path: PathBuf) -> Result<Certificate, Box<dyn Error>> {
    let content = load_file_content(der_path).await?;
    Ok(Certificate(content))
}

pub async fn load_private_key(der_path: PathBuf) -> Result<PrivateKey, Box<dyn Error>> {
    let content = load_file_content(der_path).await?;
    Ok(PrivateKey(content))
}

async fn load_file_content(path: PathBuf) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut file = File::open(path).await?;
    let mut content = vec![];
    file.read_to_end(&mut content).await?;
    Ok(content)
}
