//! 检测路径基准：流式 SHA + head/tail 读取（criterion，`cargo bench -p pxe-core` 运行）。

use criterion::{criterion_group, criterion_main, Criterion};
use std::io::Write;

fn make_sample(bytes: usize) -> std::path::PathBuf {
    let p = std::env::temp_dir().join("pxe_bench_sample.bin");
    let mut f = std::fs::File::create(&p).unwrap();
    // 伪 NSIS 布局：一块足够大的"数据"+ 尾部重复签名区，覆盖流式路径
    let block = vec![0xa5u8; 1 << 20];
    let mut written = 0;
    while written < bytes {
        let n = (bytes - written).min(block.len());
        f.write_all(&block[..n]).unwrap();
        written += n;
    }
    f.write_all(b"NullsoftInst").unwrap();
    p
}

fn bench_detect(c: &mut Criterion) {
    let p = make_sample(16 << 20); // 16MB
    c.bench_function("detect_16mb", |b| {
        b.iter(|| pxe_core::detect::detect(&p).ok())
    });
    std::fs::remove_file(&p).ok();
}

criterion_group!(benches, bench_detect);
criterion_main!(benches);
