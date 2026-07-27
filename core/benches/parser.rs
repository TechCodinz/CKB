use criterion::{criterion_group, criterion_main, Criterion};

fn parser_benchmark(_c: &mut Criterion) {
    // Benchmark placeholder for CKB AST parsing
}

criterion_group!(benches, parser_benchmark);
criterion_main!(benches);
