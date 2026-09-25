// verify: debug ok
use std::fmt::Debug;

/// A wire-format decoder. Each decoder decides its own output and error types: ASSOCIATED TYPES.
trait Decoder {
    type Output: Debug;
    type Error: Debug;
    fn decode(&self, input: &[u8]) -> Result<Self::Output, Self::Error>;
}

#[derive(Debug)]
struct Quote {
    symbol: String,
    price_cents: i64,
}

/// "SYMBOL,PRICE" text lines.
struct CsvQuoteDecoder;

#[derive(Debug)]
enum CsvError {
    NotUtf8,
    MissingField,
    BadPrice(String),
}

impl Decoder for CsvQuoteDecoder {
    type Output = Quote;
    type Error = CsvError;
    fn decode(&self, input: &[u8]) -> Result<Quote, CsvError> {
        let text = std::str::from_utf8(input).map_err(|_| CsvError::NotUtf8)?;
        let (symbol, price) = text.split_once(',').ok_or(CsvError::MissingField)?;
        let price_cents = price.trim().parse().map_err(|_| CsvError::BadPrice(price.to_string()))?;
        Ok(Quote { symbol: symbol.to_string(), price_cents })
    }
}

/// A 4-byte big-endian sequence number.
struct SeqDecoder;

impl Decoder for SeqDecoder {
    type Output = u32;
    type Error = usize; // "how many bytes were missing"
    fn decode(&self, input: &[u8]) -> Result<u32, usize> {
        let bytes: [u8; 4] = input.get(..4).ok_or(4 - input.len().min(4))?.try_into().unwrap();
        Ok(u32::from_be_bytes(bytes))
    }
}

/// Generic over ANY decoder; the output type is determined by the decoder, not chosen by the caller.
fn decode_all<D: Decoder>(decoder: &D, frames: &[&[u8]]) -> Vec<Result<D::Output, D::Error>> {
    frames.iter().map(|f| decoder.decode(f)).collect()
}

fn main() {
    let csv: [&[u8]; 3] = [b"MRDN,12550", b"ACME", b"GLBX,abc"];
    for r in decode_all(&CsvQuoteDecoder, &csv) {
        println!("{r:?}");
    }
    let seq: [&[u8]; 2] = [&[0, 0, 1, 2], &[7]];
    for r in decode_all(&SeqDecoder, &seq) {
        println!("{r:?}");
    }
}
