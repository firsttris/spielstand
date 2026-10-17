//! Random play on two or three PCs against a real server. Whatever happens (offline play,
//! missed session ends, any answer or none to conflict questions), every save state that ever
//! existed on a disk must end up as a version.
//!
//! `SPIELSTAND_SIM_CASES` sets the number of runs (CI: 24, nightly: many more).

mod common;

use common::{GAME, Pc, pc, server, server_contents};
use proptest::prelude::*;
use spielstand_agent::{Choice, Mode};

#[derive(Clone, Debug)]
enum Op {
    /// Start, play, end, through the wrapper.
    Play(usize, Choice),
    /// The disk changes, but the session end is missed (crash, power cut).
    Missed(usize),
    /// The PC loses or regains the server.
    Toggle(usize),
    /// The background service runs its catch-up.
    Background(usize),
}

fn choice() -> impl Strategy<Value = Choice> {
    prop_oneof![Just(Choice::Mine), Just(Choice::Theirs), Just(Choice::NoAnswer)]
}

fn op(pcs: usize) -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (0..pcs, choice()).prop_map(|(p, c)| Op::Play(p, c)),
        1 => (0..pcs).prop_map(Op::Missed),
        2 => (0..pcs).prop_map(Op::Toggle),
        2 => (0..pcs).prop_map(Op::Background),
    ]
}

fn scenario() -> impl Strategy<Value = (usize, Vec<Op>)> {
    (2usize..=3).prop_flat_map(|n| (Just(n), proptest::collection::vec(op(n), 1..30)))
}

async fn run(n: usize, ops: Vec<Op>) -> Result<(), String> {
    let s = server().await;
    let mut pcs: Vec<Pc> = vec![];
    for i in 0..n {
        pcs.push(pc(&s, &format!("pc{i}"), false).await);
    }
    let mut online = vec![true; n];
    let mut written: Vec<String> = vec![];
    // A change nobody looked at yet; a second change on top of it erases it before anyone could.
    let mut unobserved: Vec<Option<String>> = vec![None; n];

    for (step, op) in ops.iter().enumerate() {
        match op.clone() {
            Op::Play(p, answer) => {
                let agent = if online[p] {
                    pcs[p].agent.clone()
                } else {
                    pcs[p].offline()
                };
                pcs[p].ui.answers.lock().unwrap().push_back(answer);
                agent.prepare_launch(GAME).await;
                unobserved[p] = None;
                let content = format!("pc{p} step {step}");
                pcs[p].write_save(&content);
                written.push(content);
                agent
                    .after_session(GAME, 60)
                    .await
                    .map_err(|e| format!("step {step} {op:?}: {e:#}"))?;
            }
            Op::Missed(p) => {
                let content = format!("pc{p} step {step} unsaved");
                pcs[p].write_save(&content);
                if let Some(erased) = unobserved[p].replace(content.clone()) {
                    written.retain(|w| *w != erased);
                }
                written.push(content);
            }
            Op::Toggle(p) => online[p] = !online[p],
            Op::Background(p) => {
                if online[p] {
                    pcs[p]
                        .agent
                        .sync_game(GAME, Mode::Background)
                        .await
                        .map_err(|e| format!("step {step}: {e:#}"))?;
                    unobserved[p] = None;
                }
            }
        }
    }

    // Everyone comes back and catches up.
    for pc in &pcs {
        pc.agent
            .flush()
            .await
            .map_err(|e| format!("final flush: {e:#}"))?;
        pc.agent
            .sync_game(GAME, Mode::Background)
            .await
            .map_err(|e| format!("final sync: {e:#}"))?;
        pc.agent
            .flush()
            .await
            .map_err(|e| format!("final flush: {e:#}"))?;
    }
    let contents = server_contents(&s, GAME).await;
    for w in &written {
        if !contents.contains(w) {
            return Err(format!("lost {w:?}; server has {contents:?}"));
        }
    }
    Ok(())
}

fn cases() -> u32 {
    std::env::var("SPIELSTAND_SIM_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(24)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), max_shrink_iters: 200, ..ProptestConfig::default() })]

    #[test]
    fn no_save_state_is_ever_lost((n, ops) in scenario()) {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let result = runtime.block_on(run(n, ops));
        prop_assert!(result.is_ok(), "{}", result.unwrap_err());
    }
}
