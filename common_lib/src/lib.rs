pub trait AsSignedBytes {
    fn as_signed_bytes(&self) -> &[i8] {
        let bytes = unsafe { self.bytes().align_to() };

        bytes.1
    }

    fn bytes(&self) -> &[u8];
}

impl AsSignedBytes for &str {
    fn bytes(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl AsSignedBytes for Vec<u8> {
    fn bytes(&self) -> &[u8] {
        self.as_slice()
    }
}
