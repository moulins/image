use byteorder::{LittleEndian, ReadBytesExt};
use std::convert::TryFrom;
use std::default::Default;
use std::io::{self, Cursor, Read};
use std::marker::PhantomData;
use std::mem;

use crate::error::{DecodingError, ImageError, ImageResult};
use crate::image::{ImageDecoder, ImageFormat};

use crate::color;

use super::vp8::Frame;
use super::vp8::Vp8Decoder;

/// WebP Image format decoder. Currently only supportes the luma channel (meaning that decoded
/// images will be grayscale).
pub struct WebPDecoder<R> {
    r: R,
    frame: Frame,
    have_frame: bool,
}

impl<R: Read> WebPDecoder<R> {
    /// Create a new WebPDecoder from the Reader ```r```.
    /// This function takes ownership of the Reader.
    pub fn new(r: R) -> ImageResult<WebPDecoder<R>> {
        let f: Frame = Default::default();

        let mut decoder = WebPDecoder {
            r,
            have_frame: false,
            frame: f,
        };
        decoder.read_metadata()?;
        Ok(decoder)
    }

    fn read_riff_header(&mut self) -> ImageResult<u32> {
        let mut riff = Vec::with_capacity(4);
        self.r.by_ref().take(4).read_to_end(&mut riff)?;
        let size = self.r.read_u32::<LittleEndian>()?;
        let mut webp = Vec::with_capacity(4);
        self.r.by_ref().take(4).read_to_end(&mut webp)?;

        if &*riff != b"RIFF" {
            return Err(ImageError::Decoding(DecodingError::with_message(
                ImageFormat::WebP.into(),
                "Invalid RIFF signature".to_string(),
            )));
        }

        if &*webp != b"WEBP" {
            return Err(ImageError::Decoding(DecodingError::with_message(
                ImageFormat::WebP.into(),
                "Invalid WEBP signature".to_string(),
            )));
        }

        Ok(size)
    }

    fn read_vp8_header(&mut self) -> ImageResult<u32> {
        loop {
            let mut chunk = Vec::with_capacity(4);
            self.r.by_ref().take(4).read_to_end(&mut chunk)?;

            match &*chunk {
                b"VP8 " => {
                    let len = self.r.read_u32::<LittleEndian>()?;
                    return Ok(len);
                }
                b"ALPH" | b"VP8L" | b"ANIM" | b"ANMF" => {
                    // Alpha, Lossless and Animation isn't supported
                    return Err(ImageError::Decoding(DecodingError::with_message(
                        ImageFormat::WebP.into(),
                        "Unsupported WEBP feature.".to_string(),
                    )));
                }
                _ => {
                    let mut len = self.r.read_u32::<LittleEndian>()?;
                    if len % 2 != 0 {
                        // RIFF chunks containing an uneven number of bytes append
                        // an extra 0x00 at the end of the chunk
                        len += 1;
                    }
                    io::copy(&mut self.r.by_ref().take(len as u64), &mut io::sink())?;
                }
            }
        }
    }

    fn read_frame(&mut self, len: u32) -> ImageResult<()> {
        let mut framedata = Vec::new();
        self.r.by_ref().take(len as u64).read_to_end(&mut framedata)?;
        let m = io::Cursor::new(framedata);

        let mut v = Vp8Decoder::new(m);
        let frame = v.decode_frame()?;

        self.frame = frame.clone();

        Ok(())
    }

    fn read_metadata(&mut self) -> ImageResult<()> {
        if !self.have_frame {
            self.read_riff_header()?;
            let len = self.read_vp8_header()?;
            self.read_frame(len)?;

            self.have_frame = true;
        }

        Ok(())
    }
}

/// Wrapper struct around a `Cursor<Vec<u8>>`
pub struct WebpReader<R>(Cursor<Vec<u8>>, PhantomData<R>);
impl<R> Read for WebpReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        if self.0.position() == 0 && buf.is_empty() {
            mem::swap(buf, self.0.get_mut());
            Ok(buf.len())
        } else {
            self.0.read_to_end(buf)
        }
    }
}

impl<'a, R: 'a + Read> ImageDecoder<'a> for WebPDecoder<R> {
    type Reader = WebpReader<R>;

    fn dimensions(&self) -> (u32, u32) {
        (u32::from(self.frame.width), u32::from(self.frame.height))
    }

    fn color_type(&self) -> color::ColorType {
        color::ColorType::L8
    }

    fn into_reader(self) -> ImageResult<Self::Reader> {
        Ok(WebpReader(Cursor::new(self.frame.ybuf), PhantomData))
    }

    fn read_image(self, buf: &mut [u8]) -> ImageResult<()> {
        assert_eq!(u64::try_from(buf.len()), Ok(self.total_bytes()));
        buf.copy_from_slice(&self.frame.ybuf);
        Ok(())
    }
}
