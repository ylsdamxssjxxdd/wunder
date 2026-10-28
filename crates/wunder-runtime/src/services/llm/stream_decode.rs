use anyhow::{anyhow, Result};

#[derive(Default)]
pub(super) struct StreamDecoder {
    pending: Vec<u8>,
}

impl StreamDecoder {
    pub(super) fn push(&mut self, bytes: &[u8], output: &mut String) -> Result<()> {
        let mut offset = 0;
        while !self.pending.is_empty() && offset < bytes.len() {
            self.pending.push(bytes[offset]);
            offset += 1;
            match std::str::from_utf8(&self.pending) {
                Ok(text) => {
                    output.push_str(text);
                    self.pending.clear();
                }
                Err(error) if error.error_len().is_some() => {
                    return Err(anyhow!("LLM stream invalid UTF-8: {error}"));
                }
                Err(_) => {}
            }
        }
        match std::str::from_utf8(&bytes[offset..]) {
            Ok(text) => output.push_str(text),
            Err(error) => {
                if error.error_len().is_some() {
                    return Err(anyhow!("LLM stream invalid UTF-8: {error}"));
                }
                let valid = error.valid_up_to();
                output.push_str(std::str::from_utf8(&bytes[offset..offset + valid])?);
                self.pending.extend_from_slice(&bytes[offset + valid..]);
            }
        }
        Ok(())
    }

    pub(super) fn finish(&self) -> Result<()> {
        if !self.pending.is_empty() {
            return Err(anyhow!("LLM stream ended with incomplete UTF-8"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_utf8_boundary_roundtrips_without_unbounded_pending_bytes() {
        let original = "ascii示例✓🙂tail";
        for boundary in 0..=original.len() {
            let mut decoder = StreamDecoder::default();
            let mut output = String::new();
            decoder
                .push(&original.as_bytes()[..boundary], &mut output)
                .unwrap();
            assert!(decoder.pending.len() <= 3);
            decoder
                .push(&original.as_bytes()[boundary..], &mut output)
                .unwrap();
            decoder.finish().unwrap();
            assert_eq!(output, original);
        }
        let mut decoder = StreamDecoder::default();
        let mut output = String::new();
        for byte in original.as_bytes() {
            decoder.push(&[*byte], &mut output).unwrap();
            assert!(decoder.pending.len() <= 3);
        }
        decoder.finish().unwrap();
        assert_eq!(output, original);
    }

    #[test]
    fn invalid_and_unfinished_codepoints_are_rejected() {
        for bytes in [vec![0xff], vec![0xe4, 0x20], vec![0xe4, 0xb8], vec![0xf0]] {
            let mut decoder = StreamDecoder::default();
            let mut output = String::new();
            assert!(decoder
                .push(&bytes, &mut output)
                .and_then(|()| decoder.finish())
                .is_err());
        }
    }
}
