mod from;

use anyhow::Error;
use dockerfile_parser::Instruction::{self, *};

use super::Builder;

#[fehler::throws]
pub fn execute(builder: &Builder, instruction: &Instruction) {
    match instruction {
        From(instruction) => from::execute(builder, instruction)?,
        _ => unimplemented!("TODO"),
    }
}
