use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use rustible::utils::shell_escape;

fn bench_shell_escape(c: &mut Criterion) {
    let mut group = c.benchmark_group("shell_escape");

    // Test cases
    let simple = "simple";
    let with_space = "with space";
    let with_single_quote = "don't";
    let injection = "rm -rf /; echo 'owned'";
    let complex = "echo \"Hello World\" | grep 'Hello' > /dev/null";
    let unicode = "café";

    // Very long string
    let long_string = "a".repeat(10000);
    let long_string_with_quotes = "a'".repeat(5000);

    group.bench_function("simple", |b| {
        b.iter(|| shell_escape(black_box(simple)))
    });

    group.bench_function("with_space", |b| {
        b.iter(|| shell_escape(black_box(with_space)))
    });

    group.bench_function("with_single_quote", |b| {
        b.iter(|| shell_escape(black_box(with_single_quote)))
    });

    group.bench_function("complex_injection", |b| {
        b.iter(|| shell_escape(black_box(injection)))
    });

    group.bench_function("complex_command", |b| {
        b.iter(|| shell_escape(black_box(complex)))
    });

    group.bench_function("unicode", |b| {
        b.iter(|| shell_escape(black_box(unicode)))
    });

    group.throughput(Throughput::Bytes(long_string.len() as u64));
    group.bench_function("long_string_safe", |b| {
        b.iter(|| shell_escape(black_box(&long_string)))
    });

    group.throughput(Throughput::Bytes(long_string_with_quotes.len() as u64));
    group.bench_function("long_string_escaped", |b| {
        b.iter(|| shell_escape(black_box(&long_string_with_quotes)))
    });

    group.finish();
}

criterion_group!(benches, bench_shell_escape);
criterion_main!(benches);
