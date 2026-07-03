use std::time::Instant;
use video_pipeline::VideoPipeline;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: decode_bench <video>");
    let mut pipeline = VideoPipeline::open(std::path::Path::new(&path)).expect("open failed");
    println!("opened {} ({}x{})", path, pipeline.width, pipeline.height);

    let frame_dt = 1.0 / 30.0;
    let n = 300; // ~10s of simulated playback at 30fps redraw cadence
    let mut raw = Vec::with_capacity(n);
    let mut t = 0.0;
    for i in 0..n {
        let start = Instant::now();
        let _frame = pipeline.seek_and_decode(t, true).expect("decode failed");
        raw.push((i, start.elapsed().as_micros()));
        t += frame_dt;
    }

    let mut times: Vec<u128> = raw.iter().map(|&(_, us)| us).collect();
    times.sort_unstable();
    let sum: u128 = times.iter().sum();
    let avg = sum / times.len() as u128;
    let p50 = times[times.len() / 2];
    let p95 = times[times.len() * 95 / 100];
    let max = *times.last().unwrap();
    println!(
        "avg={}us p50={}us p95={}us max={}us over {} calls",
        avg, p50, p95, max, n
    );
    for &(i, us) in raw.iter().filter(|&&(_, us)| us > 8_000) {
        println!("  spike at call #{i}: {us}us");
    }
}
