//! How much randomness does one signature ask the host for?
//!
//! QuickJS has no `crypto`, so the sandbox draws its entropy from `__ttl_random_bytes`, a host
//! function that caps one draw at `MAX_RANDOM_BYTES` (`src/lib.rs`). That cap has to be comfortably
//! larger than a signature ever needs: a draw that was cut short would leave bytes unfilled inside
//! a single signature, which is exactly the kind of quiet difference this project keeps getting
//! bitten by.
//!
//! Counted at the host boundary rather than in JavaScript. `__ttl_random_bytes` is the only way
//! entropy enters an embedded engine, so counting its arguments measures the pool's real job and
//! cannot be fooled by the shim resolving its source somewhere in between.
//!
//! **The measured answer today is zero.** Bundle 1.0.0.388 never reaches
//! `crypto.getRandomValues` on any of the three signing paths — its per-signature variation comes
//! from `Date.now` and `Math.random`, both of which the pinned profile freezes, which is why a
//! pinned signature reproduces and an unpinned one does not. The pool exists anyway, because a
//! bundle that starts asking must not get a short or repeating answer, and this test is what
//! notices if that day comes.

use std::sync::{Arc, Mutex};

use rquickjs::{Context, Function, Runtime};

const BOOTSTRAP: &str = include_str!("../bootstrap.js");

/// Every product, because they do not use the SDK the same way.
const CASES: [(&str, &str); 3] = [
    (
        "fetch",
        "https://webcast.tiktok.com/webcast/im/fetch/?aid=1988&room_id=7300000000000000001",
    ),
    (
        "frontier",
        "https://webcast.tiktok.com/webcast/im/fetch/?aid=1988&room_id=7300000000000000001",
    ),
    (
        "ws",
        "wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/?room_id=7&aid=1988",
    ),
];

/// `MAX_RANDOM_BYTES` in `src/lib.rs`: the most one draw can return.
const DRAW_CAP: usize = 8192;

#[test]
fn one_signature_asks_for_far_less_entropy_than_the_engine_will_return() {
    let path = std::env::var("TTL_BUNDLE").unwrap_or_else(|_| "/tmp/webmssdk.js".into());
    let Ok(bundle) = std::fs::read_to_string(path) else {
        eprintln!("skipped: no signing bundle at TTL_BUNDLE or /tmp/webmssdk.js");
        return;
    };

    let asked: Arc<Mutex<Vec<usize>>> = Arc::new(Mutex::new(Vec::new()));
    let runtime = Runtime::new().expect("runtime");
    let context = Context::full(&runtime).expect("context");

    let worst = context.with(|ctx| {
        let recorder = Arc::clone(&asked);
        let random = Function::new(ctx.clone(), move |count: usize| {
            recorder.lock().expect("recorder").push(count);
            vec![7u8; count]
        })
        .expect("random function");
        ctx.globals().set("__ttl_random_bytes", random).unwrap();
        ctx.eval::<(), _>(BOOTSTRAP).expect("bootstrap");

        let prepare: Function = ctx.globals().get("ttlPrepare").unwrap();
        let _: String = prepare
            .call((bundle.as_str(), r#"{"pinned":false}"#))
            .expect("prepare the bundle");
        // Loading the bundle is a one-off, but it draws from the same source, so it counts too.
        let mut worst: usize = asked.lock().unwrap().iter().sum();
        println!("loading the bundle: {worst} bytes");

        let sign: Function = ctx.globals().get("ttlSignUrl").unwrap();
        for (product, url) in CASES {
            for _ in 0..5 {
                asked.lock().unwrap().clear();
                let _: String = sign.call((url, product)).expect("sign");
                let requests = asked.lock().unwrap();
                let total: usize = requests.iter().sum();
                println!("{product}: {requests:?} = {total} bytes");
                worst = worst.max(total);
            }
        }
        worst
    });

    println!("worst single draw: {worst} bytes; one draw is capped at {DRAW_CAP}");
    assert!(
        worst * 4 < DRAW_CAP,
        "a signature asked for {worst} bytes, which leaves no margin under the {DRAW_CAP}-byte \
         draw cap — raise MAX_RANDOM_BYTES in src/lib.rs and DRAW_CAP here"
    );
}
