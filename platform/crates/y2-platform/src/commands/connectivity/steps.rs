//! Command answers a test changes between steps. The event monitor, the self-heal and the supervisor
//! keep a clone of the root they are made with, so the root's runner stays the same while each step
//! gives it other answers; each step returns its own requests.
// SPDX-License-Identifier: GPL-2.0-only
use crate::testing::Fixture;
use std::sync::{Arc, Mutex, PoisonError};
use y2p_core::command::Outcome;

pub type Answer = (Vec<String>, Outcome);
pub type Calls = Vec<(Vec<String>, f64)>;

#[derive(Default)]
struct Script {
    answers: Vec<Answer>,
    calls: Calls,
}

#[derive(Clone, Default)]
pub struct Steps(Arc<Mutex<Script>>);

impl Steps {
    /// `fixture` with its root's requests answered by this script: the answer whose argv equals
    /// the request, anything else (also outside a step) `fixture`.
    pub fn attach(&self, fixture: Fixture) -> Fixture {
        let script = self.0.clone();
        fixture.responding(move |argv, timeout| {
            let mut script = script.lock().unwrap_or_else(PoisonError::into_inner);
            script
                .calls
                .push((argv.iter().map(|a| a.to_string()).collect(), timeout));
            script
                .answers
                .iter()
                .find(|(a, _)| a.iter().map(String::as_str).eq(argv.iter().copied()))
                .map_or_else(
                    || Outcome {
                        ok: false,
                        reason: Some("fixture".into()),
                        output: None,
                    },
                    |(_, outcome)| outcome.clone(),
                )
        })
    }

    /// Run `f` with these answers: its value and the requests it made (argv, timeout).
    pub fn run<T>(&self, answers: Vec<Answer>, f: impl FnOnce() -> T) -> (T, Calls) {
        {
            let mut script = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            script.answers = answers;
            script.calls.clear();
        }
        let value = f();
        let mut script = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        script.answers.clear();
        (value, std::mem::take(&mut script.calls))
    }
}
