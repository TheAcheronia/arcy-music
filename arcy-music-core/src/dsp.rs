pub trait AudioProcessor: Send + Sync {
    /// Process a chunk of PCM audio data. If bypassed, returns the data untouched.
    fn process(&mut self, buffer: &mut [f32]);
    fn set_bypassed(&mut self, bypassed: bool);
    fn is_bypassed(&self) -> bool;
}

pub trait SpatialRenderer: Send + Sync {
    /// Decodes an IAMF (Eclipsa) or E-AC-3 (Dolby Atmos) bitstream and renders it to a binaural stereo buffer.
    fn render_binaural(&mut self, bitstream: &[u8], out_buffer: &mut [f32]);
}
