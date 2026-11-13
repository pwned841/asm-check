//! A forward dataflow solver over the control-flow graph.
//!
//! Lints that need to know what holds *on every path* (or *on some path*)
//! reaching an instruction implement [`Analysis`] and let [`run`] iterate to
//! a fixed point before replaying each block once to report.

use std::collections::VecDeque;

use crate::diagnostic::Diagnostic;
use crate::program::{Function, Program};

pub trait Analysis {
    type State: Clone + PartialEq;

    /// The state when a function is entered.
    fn entry(&self, program: &Program, function: &Function) -> Self::State;

    /// The state for code whose predecessors cannot be determined. It must
    /// be the state under which nothing is reported.
    fn unknown(&self) -> Self::State;

    /// Combines the states of two paths that meet. Repeated joins must
    /// converge: the result may only move one way through a finite lattice.
    fn join(&self, a: &Self::State, b: &Self::State) -> Self::State;

    /// Applies the instruction at `index` to `state`. Diagnostics are pushed
    /// to `out` only on the final pass, when it is `Some`.
    fn transfer(
        &self,
        program: &Program,
        index: usize,
        state: &mut Self::State,
        out: Option<&mut Vec<Diagnostic>>,
    );
}

pub fn run<A: Analysis>(program: &Program, analysis: &A, out: &mut Vec<Diagnostic>) {
    let count = program.blocks.len();
    let mut input: Vec<Option<A::State>> = vec![None; count];
    // Blocks whose input is given rather than computed from predecessors.
    let mut pinned = vec![false; count];

    for function in &program.functions {
        input[function.entry] = Some(analysis.entry(program, function));
        pinned[function.entry] = true;
    }
    for (index, block) in program.blocks.iter().enumerate() {
        // A labelled block nothing jumps to may still be reached through a
        // jump table or from another file.
        if !block.reachable && !program.insns[block.start].labels.is_empty() {
            input[index] = Some(analysis.unknown());
            pinned[index] = true;
        }
    }

    let mut queue: VecDeque<usize> = (0..count).filter(|block| pinned[*block]).collect();
    while let Some(index) = queue.pop_front() {
        let block = &program.blocks[index];
        let Some(mut state) = input[index].clone() else { continue };
        for insn in block.start..block.end {
            analysis.transfer(program, insn, &mut state, None);
        }
        for &succ in &block.succs {
            if pinned[succ] {
                continue;
            }
            let merged = match &input[succ] {
                Some(existing) => analysis.join(existing, &state),
                None => state.clone(),
            };
            if input[succ].as_ref() != Some(&merged) {
                input[succ] = Some(merged);
                queue.push_back(succ);
            }
        }
    }

    for (index, block) in program.blocks.iter().enumerate() {
        let Some(mut state) = input[index].clone() else { continue };
        for insn in block.start..block.end {
            analysis.transfer(program, insn, &mut state, Some(out));
        }
    }
}
