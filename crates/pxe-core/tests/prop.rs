//! Property tests：detect / 流式读取对任意（含畸形）输入不得 panic。

use proptest::prelude::*;

fn tmp_file(name: &str, data: &[u8]) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("pxe_prop_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let p = d.join(name);
    std::fs::write(&p, data).unwrap();
    p
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// 任意字节序列喂给 detect 都不得 panic，且返回值自洽
    #[test]
    fn detect_never_panics(
        head in proptest::collection::vec(any::<u8>(), 0..1024),
        tail in proptest::collection::vec(any::<u8>(), 0..1024),
    ) {
        let mut data = head;
        data.extend_from_slice(&tail);
        let p = tmp_file("prop.bin", &data);
        if let Ok(d) = pxe_core::detect::detect(&p) {
            // 返回值自洽：Kind 与 label/is_installer 一致（不 panic 即主要目标）
            let _ = d.kind.label();
            let _ = d.is_installer();
        }
        std::fs::remove_file(&p).ok();
    }

    /// 变长随机文件（含超小/空文件）同样不得 panic
    #[test]
    fn detect_small_and_empty(size in 0usize..4096, seed in any::<u64>()) {
        let mut data = vec![0u8; size];
        let mut x = seed | 1;
        for b in data.iter_mut() {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *b = (x & 0xff) as u8;
        }
        let p = tmp_file("prop2.bin", &data);
        let _ = pxe_core::detect::detect(&p);
        std::fs::remove_file(&p).ok();
    }
}
