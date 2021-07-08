use std::error::Error;

pub trait Encoder: Send {
    fn calculate_max_decoded_size(&self, max_encoded_size: usize) -> usize;
    fn encode(&self, data: &[u8]) -> String;
}

pub trait Decoder: Send {
    fn decode(&self, data: String) -> Result<Vec<u8>, Box<dyn Error>>;
}

#[derive(Clone, Copy)]
pub struct HexEncoder {}

impl Encoder for HexEncoder {
    fn calculate_max_decoded_size(&self, max_encoded_size: usize) -> usize {
        max_encoded_size / 2
    }
    fn encode(&self, data: &[u8]) -> String {
        hex::encode(data)
    }
}

impl Decoder for HexEncoder {
    fn decode(&self, data: String) -> Result<Vec<u8>, Box<dyn Error>> {
        match hex::decode(data) {
            Ok(v) => Ok(v),
            Err(e) => Err(e.into()),
        }
    }
}
