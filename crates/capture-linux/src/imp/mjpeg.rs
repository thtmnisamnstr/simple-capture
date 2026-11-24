//! MJPEG camera frames to NV12 through GStreamer's jpegdec (gst-plugins-good), for cameras with
//! no raw mode.

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use sc_geometry::SizePx;
use sc_platform::{CpuNv12Frame, Error, Result};

pub struct Mjpeg {
    pipeline: gst::Pipeline,
    src: gst_app::AppSrc,
    sink: gst_app::AppSink,
    size: SizePx,
}

impl Mjpeg {
    pub fn new(size: SizePx) -> Result<Mjpeg> {
        gst::init().map_err(|e| Error::Other(format!("GStreamer: {e}")))?;
        if gst::ElementFactory::find("jpegdec").is_none() {
            return Err(Error::Unsupported(
                "this camera only sends MJPEG, which needs GStreamer's jpegdec (gstreamer1.0-plugins-good)",
            ));
        }
        let desc = format!(
            "appsrc name=src is-live=true format=time caps=image/jpeg,width={w},height={h},framerate=0/1 \
             ! jpegdec ! videoconvert ! videoscale \
             ! video/x-raw,format=NV12,width={w},height={h} \
             ! appsink name=sink sync=false",
            w = size.w,
            h = size.h,
        );
        let pipeline = gst::parse::launch(&desc)
            .map_err(|e| Error::Other(format!("MJPEG pipeline: {e}")))?
            .downcast::<gst::Pipeline>()
            .map_err(|_| Error::Other("MJPEG pipeline: not a pipeline".into()))?;
        let by = |name: &str| {
            pipeline
                .by_name(name)
                .ok_or_else(|| Error::Other(format!("MJPEG pipeline: no {name}")))
        };
        let src = by("src")?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| Error::Other("MJPEG pipeline: src is not an appsrc".into()))?;
        let sink = by("sink")?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| Error::Other("MJPEG pipeline: sink is not an appsink".into()))?;
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| Error::Other(format!("MJPEG pipeline: {e}")))?;
        Ok(Mjpeg {
            pipeline,
            src,
            sink,
            size,
        })
    }

    pub fn decode(&mut self, jpeg: &[u8]) -> Option<CpuNv12Frame> {
        self.src
            .push_buffer(gst::Buffer::from_slice(jpeg.to_vec()))
            .ok()?;
        let sample = self
            .sink
            .try_pull_sample(gst::ClockTime::from_mseconds(500))?;
        let map = sample.buffer()?.map_readable().ok()?;
        let stride = (self.size.w as usize + 3) & !3;
        let need = stride * (self.size.h as usize + (self.size.h as usize).div_ceil(2));
        (map.len() >= need).then(|| super::camera::nv12_copy(&map, stride, self.size))
    }
}

impl Drop for Mjpeg {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg(w: u32, h: u32) -> Option<Vec<u8>> {
        gst::init().ok()?;
        gst::ElementFactory::find("jpegenc")?;
        let p = gst::parse::launch(&format!(
            "videotestsrc num-buffers=1 pattern=smpte ! video/x-raw,width={w},height={h} \
             ! jpegenc ! appsink name=out sync=false"
        ))
        .ok()?
        .downcast::<gst::Pipeline>()
        .ok()?;
        let out = p.by_name("out")?.downcast::<gst_app::AppSink>().ok()?;
        p.set_state(gst::State::Playing).ok()?;
        let sample = out.try_pull_sample(gst::ClockTime::from_seconds(5));
        let _ = p.set_state(gst::State::Null);
        Some(sample?.buffer()?.map_readable().ok()?.to_vec())
    }

    #[test]
    fn an_mjpeg_frame_becomes_nv12() {
        let size = SizePx { w: 64, h: 48 };
        let Some(jpg) = jpeg(size.w, size.h) else {
            eprintln!("SKIPPED: no GStreamer jpegenc here");
            return;
        };
        let mut m = Mjpeg::new(size).expect("decoder");
        let f = m.decode(&jpg).expect("a frame");
        assert_eq!(f.y.len(), 64 * 48);
        assert!(
            f.y.iter().any(|v| *v > 100),
            "the test pattern has bright bars"
        );
        assert!(m.decode(&jpg).is_some(), "a second frame decodes too");
    }
}
