use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tuxtalks_oxide::utils::fuzzy::find_matches;

fn bench_fuzzy_matching(c: &mut Criterion) {
    let candidates = [
        "Dark Side of the Moon",
        "The Wall",
        "Wish You Were Here",
        "Animals",
        "Meddle",
        "The Piper at the Gates of Dawn",
        "A Saucerful of Secrets",
        "More",
        "Ummagumma",
        "Atom Heart Mother",
    ];

    c.bench_function("fuzzy_match_short", |b| {
        b.iter(|| {
            find_matches(
                black_box("Dark Side"),
                black_box(&candidates),
                black_box(5),
                black_box(0.6),
            )
        });
    });
}

criterion_group!(benches, bench_fuzzy_matching);
criterion_main!(benches);
