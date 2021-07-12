use std::error::Error;

use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::convert::TryInto;

pub trait Encoder: Send {
    fn calculate_max_decoded_size(&self, max_encoded_size: usize) -> usize;
    fn encode(&self, data: &[u8]) -> String;
}

pub trait Decoder: Send {
    fn decode(&self, data: &str) -> Result<Vec<u8>, Box<dyn Error>>;
}

pub struct HexEncoder {}

impl Encoder for HexEncoder {
    fn calculate_max_decoded_size(&self, max_encoded_size: usize) -> usize {
        max_encoded_size / 2
    }
    fn encode(&self, data: &[u8]) -> String {
        hex::encode(data)
    }
}

pub struct HexDecoder {}

impl Decoder for HexDecoder {
    fn decode(&self, data: &str) -> Result<Vec<u8>, Box<dyn Error>> {
        match hex::decode(data) {
            Ok(v) => Ok(v),
            Err(e) => Err(e.into()),
        }
    }
}

pub const CLIENT_ID_SIZE_IN_BYTES: usize = 4;
pub type ClientId = [u8; CLIENT_ID_SIZE_IN_BYTES];

pub fn new_client_id() -> ClientId {
    let values: Vec<u8> = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(CLIENT_ID_SIZE_IN_BYTES)
        .collect();
    values.try_into().unwrap()
}

pub struct ClientIdSuffixEncoder<E: Encoder> {
    encoder: E,
}

impl<E: Encoder> ClientIdSuffixEncoder<E> {
    pub fn new(encoder: E) -> Self {
        ClientIdSuffixEncoder { encoder }
    }
}

impl<E: Encoder> Encoder for ClientIdSuffixEncoder<E> {
    fn calculate_max_decoded_size(&self, max_encoded_size: usize) -> usize {
        self.encoder.calculate_max_decoded_size(max_encoded_size) - CLIENT_ID_SIZE_IN_BYTES
    }

    fn encode(&self, data: &[u8]) -> String {
        let (data, client_id) = data.split_at(data.len() - CLIENT_ID_SIZE_IN_BYTES);
        let mut res = self.encoder.encode(data);
        let encoded_client_id = std::str::from_utf8(client_id).unwrap();
        println!("{:?}   {:?}", data, client_id);
        res.push_str(encoded_client_id);
        res
    }
}

pub struct ClientIdSuffixDecoder<D: Decoder> {
    decoder: D,
}

impl<D: Decoder> ClientIdSuffixDecoder<D> {
    pub fn new(decoder: D) -> Self {
        ClientIdSuffixDecoder { decoder }
    }
}

impl<D: Decoder> Decoder for ClientIdSuffixDecoder<D> {
    fn decode(&self, data: &str) -> Result<Vec<u8>, Box<dyn Error>> {
        let (data, client_id) = data.split_at(data.len() - CLIENT_ID_SIZE_IN_BYTES);
        let mut res = self.decoder.decode(data)?;
        let decoded_client_id = client_id.as_bytes();
        res.extend_from_slice(decoded_client_id);
        Ok(res)
    }
}
