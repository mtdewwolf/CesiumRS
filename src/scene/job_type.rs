//! CesiumJS `JobType`: the kinds of work `JobScheduler` budgets per frame.

numeric_enum!(JobType: i32 {
    Texture = 0 => "TEXTURE",
    Program = 1 => "PROGRAM",
    Buffer = 2 => "BUFFER",
});

impl JobType {
    pub const NUMBER_OF_JOB_TYPES: i32 = 3;
}
