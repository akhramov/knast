use anyhow::Error;
use dockerfile_parser::FromInstruction;

use super::Builder;

#[fehler::throws]
pub fn execute(builder: &Builder, instruction: &FromInstruction) {

}
