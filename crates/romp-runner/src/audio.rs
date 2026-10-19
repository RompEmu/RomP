use cubeb::{
    ChannelLayout, Context, SampleFormat, StereoFrame, StreamBuilder, StreamParamsBuilder,
};
use ringbuf::traits::{Consumer, Producer};
use ringbuf::{Cons, HeapRb, Prod};
use std::ffi::CString;
use std::sync::Arc;
use tracing::{debug, info, warn};

pub type AudioProducer = Prod<Arc<HeapRb<i16>>>;
type AudioConsumer = Cons<Arc<HeapRb<i16>>>;

pub struct AudioOutput {
    pub rate: u32,
    _stream: cubeb::Stream<StereoFrame<i16>>,
    _ctx: Context,
}

/// Opens the device at its own rate, so RomP does any conversion rather than the system: Windows
/// warbles when handed a rate such as mGBA's 131072 Hz.
pub fn open(core_rate: u32, buffer_ms: usize) -> anyhow::Result<(AudioOutput, AudioProducer)> {
    let context_name = CString::new("RomP")?;
    let ctx = Context::init(Some(context_name.as_c_str()), None)?;
    let sample_rate = match ctx.preferred_sample_rate() {
        Ok(rate) if rate > 0 => rate,
        Ok(_) => core_rate,
        Err(e) => {
            warn!("preferred_sample_rate failed: {e}; using the core's {core_rate} Hz");
            core_rate
        }
    };
    let capacity_samples = sample_rate as usize * 2 * buffer_ms / 1000;
    let params = StreamParamsBuilder::new()
        .format(SampleFormat::S16NE)
        .rate(sample_rate)
        .channels(2)
        .layout(ChannelLayout::STEREO)
        .take();
    let min_latency = match ctx.min_latency(&params) {
        Ok(l) => l,
        Err(e) => {
            warn!("min_latency failed: {e}; using 1024");
            1024
        }
    };

    let rb = Arc::new(HeapRb::<i16>::new(capacity_samples));
    let mut prod: AudioProducer = Prod::new(rb.clone());
    let mut cons: AudioConsumer = Cons::new(rb);

    let prefill_frames = (sample_rate as usize / 10).max(2048);
    let silence = vec![0i16; prefill_frames * 2];
    let pushed = prod.push_slice(&silence);
    info!(
        prefill_samples = pushed,
        "audio ring prefilled with silence"
    );

    let mut builder = StreamBuilder::<StereoFrame<i16>>::new();
    builder
        .name(CString::new("RomP")?)
        .default_output(&params)
        .latency(min_latency.max(1024))
        .data_callback(
            move |_input: &[StereoFrame<i16>], output: &mut [StereoFrame<i16>]| -> isize {
                const _: () = assert!(std::mem::size_of::<StereoFrame<i16>>() == 4);
                let frames = output.len() as isize;
                let samples = output.len() * 2;
                let interleaved: &mut [i16] = unsafe {
                    std::slice::from_raw_parts_mut(output.as_mut_ptr() as *mut i16, samples)
                };
                let popped = cons.pop_slice(interleaved);
                interleaved[popped..].fill(0);
                frames
            },
        )
        .state_callback(|state| {
            debug!("cubeb state: {:?}", state);
        });
    let stream = builder.init(&ctx)?;
    stream.start()?;
    info!(
        rate = sample_rate,
        core_rate,
        latency = min_latency,
        "cubeb stream started"
    );
    Ok((
        AudioOutput {
            rate: sample_rate,
            _stream: stream,
            _ctx: ctx,
        },
        prod,
    ))
}
