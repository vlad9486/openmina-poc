use redux::ActionWithMeta;

use super::{State, Action};

impl State {
    pub fn reducer(&mut self, action: &ActionWithMeta<Action>) {
        match action.action() {
            Action::Continue(block) => {
                self.blocks.push(block.clone());
            }
            Action::Apply(_) => {
                self.blocks.pop();
            }
        }
    }
}
