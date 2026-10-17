//! Random play on two or three PCs against a real server. Whatever happens (offline play,
//! missed session ends, any answer or none to conflict questions, another program copying
//! saves between the PCs), every save state an agent ever saw must end up as a version. In
//! backup mode, additionally, the agent must never change a save on any disk.
//!
//! `SPIELSTAND_SIM_CASES` sets the number of runs (CI: 64, nightly: thousands).

mod common;

use common::{GAME, Pc, pc, server, server_contents};
use proptest::prelude::*;
use spielstand_agent::{Choice, Mode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Run {
    Sync,
    Backup,
}

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
    /// The file watcher saw a change and the files went quiet.
    Observe(usize),
    /// Another program (Stellwerk, Syncthing, a copy by hand) copies the save from one PC to
    /// another, whatever either of them holds.
    Copy(usize, usize),
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
        2 => (0..pcs).prop_map(Op::Observe),
        2 => (0..pcs, 0..pcs).prop_map(|(a, b)| Op::Copy(a, b)),
    ]
}

fn scenario() -> impl Strategy<Value = (Run, usize, Vec<Op>)> {
    (prop_oneof![Just(Run::Sync), Just(Run::Backup)], 2usize..=3)
        .prop_flat_map(|(run, n)| (Just(run), Just(n), proptest::collection::vec(op(n), 1..30)))
}

struct World {
    run: Run,
    pcs: Vec<Pc>,
    online: Vec<bool>,
    /// Every state an agent must keep.
    written: Vec<String>,
    /// A change nobody looked at yet; overwriting it erases it before anyone could.
    unobserved: Vec<Option<String>>,
}

impl World {
    fn agent(&self, p: usize) -> spielstand_agent::Agent {
        if self.online[p] {
            self.pcs[p].agent.clone()
        } else {
            self.pcs[p].offline()
        }
    }

    /// Something not the agent replaces the save on `p`.
    fn replace(&mut self, p: usize, content: &str, new_state: bool) {
        if let Some(erased) = self.unobserved[p].take() {
            self.written.retain(|w| *w != erased);
        }
        self.pcs[p].write_save(content);
        if new_state {
            self.written.push(content.to_string());
            self.unobserved[p] = Some(content.to_string());
        }
    }

    /// Runs an agent step; in backup mode the disk must be the same afterwards.
    async fn agent_step<F, Fut>(&mut self, p: usize, label: &str, step: F) -> Result<(), String>
    where
        F: FnOnce(spielstand_agent::Agent) -> Fut,
        Fut: std::future::Future<Output = Result<(), String>>,
    {
        let before = self.pcs[p].read_save();
        step(self.agent(p)).await.map_err(|e| format!("{label}: {e}"))?;
        self.unobserved[p] = None;
        if self.run == Run::Backup && self.pcs[p].read_save() != before {
            return Err(format!(
                "{label}: backup mode changed pc{p}'s save from {before:?} to {:?}",
                self.pcs[p].read_save()
            ));
        }
        Ok(())
    }
}

async fn run(run: Run, n: usize, ops: Vec<Op>) -> Result<(), String> {
    let s = server().await;
    let mut pcs: Vec<Pc> = vec![];
    for i in 0..n {
        pcs.push(pc(&s, &format!("pc{i}"), false).await);
    }
    if run == Run::Backup {
        let refs: Vec<&Pc> = pcs.iter().collect();
        s.set_mode(GAME, Some("backup"), &refs).await;
    }
    let mut w = World {
        run,
        pcs,
        online: vec![true; n],
        written: vec![],
        unobserved: vec![None; n],
    };

    for (step, op) in ops.iter().enumerate() {
        let label = format!("step {step} {op:?}");
        match op.clone() {
            Op::Play(p, answer) => {
                w.pcs[p].ui.answers.lock().unwrap().push_back(answer);
                w.agent_step(p, &label, |a| async move {
                    a.prepare_launch(GAME).await;
                    Ok(())
                })
                .await?;
                let content = format!("pc{p} step {step}");
                w.replace(p, &content, true);
                w.agent_step(p, &label, |a| async move {
                    a.after_session(GAME, 60)
                        .await
                        .map(|_| ())
                        .map_err(|e| format!("{e:#}"))
                })
                .await?;
            }
            Op::Missed(p) => {
                let content = format!("pc{p} step {step} unsaved");
                w.replace(p, &content, true);
            }
            Op::Toggle(p) => w.online[p] = !w.online[p],
            Op::Background(p) if w.online[p] => {
                w.agent_step(p, &label, |a| async move {
                    a.sync_game(GAME, Mode::Background)
                        .await
                        .map(|_| ())
                        .map_err(|e| format!("{e:#}"))
                })
                .await?;
            }
            Op::Background(_) => {}
            Op::Observe(p) => {
                w.agent_step(p, &label, |a| async move {
                    a.observe(GAME).await.map(|_| ()).map_err(|e| format!("{e:#}"))
                })
                .await?;
            }
            Op::Copy(a, b) if a != b => {
                if let Some(content) = w.pcs[a].read_save() {
                    w.replace(b, &content, false);
                }
            }
            Op::Copy(..) => {}
        }
    }

    // Everyone comes back and catches up.
    for p in 0..n {
        w.online[p] = true;
        let label = format!("final pc{p}");
        w.agent_step(p, &label, |a| async move {
            a.flush().await.map_err(|e| format!("{e:#}"))?;
            a.sync_game(GAME, Mode::Background)
                .await
                .map_err(|e| format!("{e:#}"))?;
            a.flush().await.map(|_| ()).map_err(|e| format!("{e:#}"))
        })
        .await?;
    }
    let contents = server_contents(&s, GAME).await;
    for kept in &w.written {
        if !contents.contains(kept) {
            return Err(format!("lost {kept:?}; server has {contents:?}"));
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
    fn no_save_state_is_ever_lost((mode, n, ops) in scenario()) {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let result = runtime.block_on(run(mode, n, ops));
        prop_assert!(result.is_ok(), "{}", result.unwrap_err());
    }
}
