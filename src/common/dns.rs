use std::error::Error;

use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::convert::TryInto;

pub trait Encoder: Send {
    fn calculate_max_decoded_size(&self, max_encoded_size: usize) -> usize;
    fn encode(&self, data: &[u8]) -> Result<String, Box<dyn Error>>;
}

pub trait Decoder: Send {
    fn decode(&self, data: &str) -> Result<Vec<u8>, Box<dyn Error>>;
}

pub struct HexEncoder {}

impl Encoder for HexEncoder {
    fn calculate_max_decoded_size(&self, max_encoded_size: usize) -> usize {
        max_encoded_size / 2
    }
    fn encode(&self, data: &[u8]) -> Result<String, Box<dyn Error>> {
        Ok(hex::encode(data))
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
        Self { encoder }
    }
}

impl<E: Encoder> Encoder for ClientIdSuffixEncoder<E> {
    fn calculate_max_decoded_size(&self, max_encoded_size: usize) -> usize {
        self.encoder.calculate_max_decoded_size(max_encoded_size) - CLIENT_ID_SIZE_IN_BYTES
    }

    fn encode(&self, data: &[u8]) -> Result<String, Box<dyn Error>> {
        if data.len() < CLIENT_ID_SIZE_IN_BYTES {
            return Err(String::from("not enough data to encode").into());
        }

        let (data, client_id) = data.split_at(data.len() - CLIENT_ID_SIZE_IN_BYTES);
        let mut res = self.encoder.encode(data)?;
        let encoded_client_id = std::str::from_utf8(client_id).unwrap();
        res.push_str(encoded_client_id);
        Ok(res)
    }
}

pub struct ClientIdSuffixDecoder<D: Decoder> {
    decoder: D,
}

impl<D: Decoder> ClientIdSuffixDecoder<D> {
    pub fn new(decoder: D) -> Self {
        Self { decoder }
    }
}

impl<D: Decoder> Decoder for ClientIdSuffixDecoder<D> {
    fn decode(&self, data: &str) -> Result<Vec<u8>, Box<dyn Error>> {
        if data.len() < CLIENT_ID_SIZE_IN_BYTES {
            return Err(String::from("not enough data to decode").into());
        }

        let (data, client_id) = data.split_at(data.len() - CLIENT_ID_SIZE_IN_BYTES);
        let mut res = self.decoder.decode(data)?;
        let decoded_client_id = client_id.as_bytes();
        res.extend_from_slice(decoded_client_id);
        Ok(res)
    }
}

pub struct AppendSuffixEncoder<E: Encoder> {
    encoder: E,
    suffix: String,
}

impl<E: Encoder> AppendSuffixEncoder<E> {
    pub fn new(encoder: E, suffix: String) -> Self {
        Self { encoder, suffix }
    }
}

impl<E: Encoder> Encoder for AppendSuffixEncoder<E> {
    fn calculate_max_decoded_size(&self, max_encoded_size: usize) -> usize {
        self.encoder.calculate_max_decoded_size(max_encoded_size) - self.suffix.len()
    }

    fn encode(&self, data: &[u8]) -> Result<String, Box<dyn Error>> {
        let mut res = self.encoder.encode(data)?;
        res.push_str(self.suffix.as_str());
        Ok(res)
    }
}

pub struct AppendSuffixDecoder<D: Decoder> {
    decoder: D,
    suffix: String,
}

impl<D: Decoder> AppendSuffixDecoder<D> {
    pub fn new(decoder: D, suffix: String) -> Self {
        Self { decoder, suffix }
    }
}

impl<D: Decoder> Decoder for AppendSuffixDecoder<D> {
    fn decode(&self, data: &str) -> Result<Vec<u8>, Box<dyn Error>> {
        let data_without_suffix = data
            .strip_suffix(self.suffix.as_str())
            .ok_or_else(|| String::from("missing suffix"))?;
        self.decoder.decode(data_without_suffix)
    }
}

#[cfg(test)]
mod tests {

    use mockall::mock;

    use super::*;

    mock! {
        Encoder{}
        impl Encoder for Encoder {
            fn calculate_max_decoded_size(&self, max_encoded_size: usize) -> usize;
            fn encode(&self, data: &[u8]) -> Result<String, Box<dyn Error>>;
        }
    }

    mock! {
        Decoder{}
        impl Decoder for Decoder {
            fn decode(&self, data: &str) -> Result<Vec<u8>, Box<dyn Error>>;
        }
    }

    #[test]
    fn client_id_suffix_encoder_not_enough_data() -> Result<(), Box<dyn Error>> {
        let encoder_mock = MockEncoder::new();
        let encoder = ClientIdSuffixEncoder::new(encoder_mock);
        let res = encoder.encode(b"123");
        assert!(res.is_err());
        Ok(())
    }

    #[test]
    fn client_id_suffix_encoder_internal_encoder_failed() -> Result<(), Box<dyn Error>> {
        let mut encoder_mock = MockEncoder::new();
        encoder_mock
            .expect_encode()
            .returning(|_| Err(String::from("bla").into()));
        let encoder = ClientIdSuffixEncoder::new(encoder_mock);
        let res = encoder.encode(b"bla1234");
        assert!(res.is_err());
        Ok(())
    }

    #[test]
    fn client_id_suffix_encoder_success() -> Result<(), Box<dyn Error>> {
        let mut encoder_mock = MockEncoder::new();
        encoder_mock
            .expect_encode()
            .returning(|_| Ok(String::from("encoded")));
        let encoder = ClientIdSuffixEncoder::new(encoder_mock);
        let res = encoder.encode(b"bla1234")?;
        assert_eq!("encoded1234", res);
        Ok(())
    }

    #[test]
    fn client_id_suffix_decoder_not_enough_data() -> Result<(), Box<dyn Error>> {
        let decoder_mock = MockDecoder::new();
        let decoder = ClientIdSuffixDecoder::new(decoder_mock);
        let res = decoder.decode("123");
        assert!(res.is_err());
        Ok(())
    }

    #[test]
    fn client_id_suffix_decoder_internal_decoder_failed() -> Result<(), Box<dyn Error>> {
        let mut decoder_mock = MockDecoder::new();
        decoder_mock
            .expect_decode()
            .returning(|_| Err(String::from("bla").into()));
        let decoder = ClientIdSuffixDecoder::new(decoder_mock);
        let res = decoder.decode("bla1234");
        assert!(res.is_err());
        Ok(())
    }

    #[test]
    fn client_id_suffix_decoder_success() -> Result<(), Box<dyn Error>> {
        let mut decoder_mock = MockDecoder::new();
        decoder_mock
            .expect_decode()
            .returning(|_| Ok(String::from("decoded").into_bytes()));
        let decoder = ClientIdSuffixDecoder::new(decoder_mock);
        let res = decoder.decode("bla1234")?;
        assert_eq!(b"decoded1234", res.as_slice());
        Ok(())
    }

    #[test]
    fn append_suffix_encoder_internal_encoder_failed() -> Result<(), Box<dyn Error>> {
        let suffix = String::from("xyz");
        let mut encoder_mock = MockEncoder::new();
        encoder_mock
            .expect_encode()
            .returning(|_| Err(String::from("bla").into()));
        let encoder = AppendSuffixEncoder::new(encoder_mock, suffix);
        let res = encoder.encode(b"bla");
        assert!(res.is_err());
        Ok(())
    }

    #[test]
    fn append_suffix_encoder_success() -> Result<(), Box<dyn Error>> {
        let suffix = String::from("xyz");
        let mut encoder_mock = MockEncoder::new();
        encoder_mock
            .expect_encode()
            .returning(|_| Ok(String::from("encoded")));
        let encoder = AppendSuffixEncoder::new(encoder_mock, suffix);
        let res = encoder.encode(b"bla")?;
        assert_eq!("encodedxyz", res);
        Ok(())
    }

    #[test]
    fn append_suffix_decoder_missing_suffix() -> Result<(), Box<dyn Error>> {
        let suffix = String::from("xyz");
        let decoder_mock = MockDecoder::new();
        let decoder = AppendSuffixDecoder::new(decoder_mock, suffix);
        let res = decoder.decode("123");
        assert!(res.is_err());
        Ok(())
    }

    #[test]
    fn append_suffix_decoder_internal_decoder_failed() -> Result<(), Box<dyn Error>> {
        let suffix = String::from("xyz");
        let mut decoder_mock = MockDecoder::new();
        decoder_mock
            .expect_decode()
            .returning(|_| Err(String::from("bla").into()));
        let decoder = AppendSuffixDecoder::new(decoder_mock, suffix);
        let res = decoder.decode("blaxyz");
        assert!(res.is_err());
        Ok(())
    }

    #[test]
    fn append_suffix_decoder_success() -> Result<(), Box<dyn Error>> {
        let suffix = String::from("xyz");
        let mut decoder_mock = MockDecoder::new();
        decoder_mock
            .expect_decode()
            .returning(|_| Ok(String::from("decoded").into_bytes()));
        let decoder = AppendSuffixDecoder::new(decoder_mock, suffix);
        let res = decoder.decode("blaxyz")?;
        assert_eq!(b"decoded", res.as_slice());
        Ok(())
    }
}
