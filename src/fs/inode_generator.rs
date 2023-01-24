use fuser::FUSE_ROOT_ID;

#[derive(Debug)]
pub struct INodeGenerator {
    last_value: u64
}

impl INodeGenerator {
    pub(crate) fn new() -> Self {
        Self {
            last_value: FUSE_ROOT_ID,
        }
    }

    pub(crate) fn next(&mut self) -> u64 {
        self.last_value += 1;
        self.last_value
    }
}
