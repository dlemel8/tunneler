use std::error::Error;
use std::fmt::Debug;
use std::io::BufReader;
use std::path::PathBuf;

use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio_rustls::rustls::internal::pemfile;
use tokio_rustls::rustls::sign::{RSASigningKey, SigningKey};
use tokio_rustls::rustls::{Certificate, PrivateKey};

pub async fn load_certificate(pem_path: PathBuf) -> Result<Certificate, Box<dyn Error>> {
    log::debug!("loading certificate from {:?}", pem_path);
    let content = load_file_content(pem_path).await?;
    let parsed_result = pemfile::certs(&mut BufReader::new(content.as_slice()));
    parse_pem_single_element_result(parsed_result)
}

pub async fn load_private_key(pem_path: PathBuf) -> Result<PrivateKey, Box<dyn Error>> {
    log::debug!("loading private key from {:?}", pem_path);
    let content = load_file_content(pem_path).await?;
    let parsed_result = pemfile::pkcs8_private_keys(&mut BufReader::new(content.as_slice()));

    if let Ok(key) = parse_pem_single_element_result(parsed_result) {
        return Ok(key);
    };

    let parsed_result = pemfile::rsa_private_keys(&mut BufReader::new(content.as_slice()));
    parse_pem_single_element_result(parsed_result)
}

pub async fn load_signing_key(pem_path: PathBuf) -> Result<Box<dyn SigningKey>, Box<dyn Error>> {
    let private_key = load_private_key(pem_path).await?;
    match RSASigningKey::new(&private_key) {
        Ok(s) => Ok(Box::new(s)),
        Err(_) => Err("failed to convert private key to signing key".into()),
    }
}

fn parse_pem_single_element_result<T: Clone + Debug>(
    result: Result<Vec<T>, ()>,
) -> Result<T, Box<dyn Error>> {
    match result {
        Ok(vec) => match vec.len() {
            1 => Ok(vec.get(0).unwrap().clone()),
            _ => Err(format!("expected a single element, got {:?}", vec).into()),
        },
        Err(_) => Err("failed to parse PEM content".into()),
    }
}

async fn load_file_content(path: PathBuf) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut file = File::open(path).await?;
    let mut content = vec![];
    file.read_to_end(&mut content).await?;
    Ok(content)
}
