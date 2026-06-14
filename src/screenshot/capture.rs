use clap::Args;
use wayland_client::protocol::wl_shm_pool;
use std::os::fd::AsFd;
use std::path::PathBuf;
use wayland_client::{protocol::{wl_registry, wl_output, wl_buffer, wl_shm}, Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1, zwlr_screencopy_manager_v1
};
use wl_clipboard_rs::copy::{MimeType, Options, Source};

#[derive(Args, Debug)]
pub struct CaptureArgs {
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long)]
    clipboard: bool,
    #[arg(long)]
    x: Option<i32>,
    #[arg(long)]
    y: Option<i32>,
    #[arg(long)]
    width: Option<i32>,
    #[arg(long)]
    height: Option<i32>,
}

#[derive(Default)]
struct State {
    shm: Option<wl_shm::WlShm>,
    output: Option<wl_output::WlOutput>,
    manager: Option<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1>,
    buffer: Option<wl_buffer::WlBuffer>,
    mmap: Option<memmap2::MmapMut>,
    width: u32,
    height: u32,
    stride: u32,
    done: bool,
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: <wl_registry::WlRegistry as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    )
    {
        if let wl_registry::Event::Global { name, interface, .. } = event {
            match interface.as_str() {
                "wl_shm" => state.shm = Some(registry.bind(name, 1, qh, ())),
                "wl_output" => state.output = Some(registry.bind(name, 1, qh, ())),
                "zwlr_screencopy_manager_v1" => state.manager = Some(registry.bind(name, 1, qh, ())),
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_shm::WlShm, ()> for State {
    fn event(
        _: &mut Self,
        _: &wl_shm::WlShm,
        _: <wl_shm::WlShm as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    )
    {

    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for State {
    fn event(
        _: &mut Self,
        _: &wl_shm_pool::WlShmPool,
        _: <wl_shm_pool::WlShmPool as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    )
    {

    }
}

impl Dispatch<wl_output::WlOutput, ()> for State {
    fn event(
        _: &mut Self,
        _: &wl_output::WlOutput,
        _: <wl_output::WlOutput as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    )
    {

    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for State {
    fn event(
        _: &mut Self,
        _: &wl_buffer::WlBuffer,
        _: <wl_buffer::WlBuffer as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    )
    {

    }
}

impl Dispatch<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
        _: <zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    )
    {

    }
}

impl Dispatch<zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1, ()> for State {
    fn event(
        state: &mut Self,
        frame: &zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1,
        event: <zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    )
    {
        use zwlr_screencopy_frame_v1::Event;
        match event {
            Event::Buffer { format, width, height, stride } => {
                use rustix::shm::OFlags;

                state.width = width;
                state.height = height;
                state.stride = stride;

                let size = (stride * height) as usize;
                let shm_name = format!("/leta-{}", std::process::id());
                let fd = rustix::shm::open(&shm_name, OFlags::CREATE | OFlags::RDWR | OFlags::EXCL, rustix::fs::Mode::from_bits_truncate(0o600)).expect("shm_open failed");
                rustix::shm::unlink(&shm_name).ok();
                rustix::fs::ftruncate(&fd, size as u64).expect("ftruncate failed");

                let mmap = unsafe { memmap2::MmapOptions::new().len(size).map_mut(&fd).expect("mmap failed") };

                let pool = state.shm.as_ref().unwrap().create_pool(fd.as_fd(), size as i32, qh, ());
                let buffer = pool.create_buffer(0, width as i32, height as i32, stride as i32, format.into_result().unwrap(), qh, ());
                pool.destroy();

                frame.copy(&buffer);

                state.buffer = Some(buffer);
                state.mmap = Some(mmap);
            }
            Event::Ready { .. } => {
                println!("Ready");
                state.done = true;
            }
            Event::Failed => {
                println!("Failed");
                state.done = false;
            }
            other => println!("Other event {:?}", other)
        }
    }
}

pub fn run(args: CaptureArgs) -> anyhow::Result<()> {
    let conn = Connection::connect_to_env()?;
    let display = conn.display();

    let mut event_queue = conn.new_event_queue::<State>();
    let qh = event_queue.handle();

    display.get_registry(&qh, ());

    let mut state = State::default();
    event_queue.roundtrip(&mut state)?;

    let manager = state.manager.as_ref().expect("no screencopy manager");
    let output = state.output.as_ref().expect("no output");

    match (args.x, args.y, args.width, args.height) {
        (Some(x), Some(y), Some(width), Some(height)) => {
            manager.capture_output_region(0, output, x, y, width, height, &qh, ());
        }
        _ => {
            manager.capture_output(0, output, &qh, ());
        }
    }

    while !state.done {
        event_queue.blocking_dispatch(&mut state)?;
    }

    let pixels = state.mmap.as_ref().unwrap().to_vec();
    let width = state.width;
    let height = state.height;

    if args.clipboard {
        let img = image::RgbaImage::from_raw_bgra(width, height, pixels).expect("failed to construct image");

        if let Some(output) = args.output {
            img.save(&output)?;
        }

        let mut png_bytes: Vec<u8> = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)?;

        let mut opts = Options::new();
        opts.foreground(true);
        opts.copy(Source::Bytes(png_bytes.into()), MimeType::Specific("image/png".to_string()))?;
    } else {
        let output = args.output.expect("--output required when not using --clipboard");
        let img = image::RgbaImage::from_raw_bgra(width, height, pixels).expect("failed to construct image");
        img.save(&output)?;
    }

    Ok(())
}
