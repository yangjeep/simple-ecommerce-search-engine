mod contracts;
mod execution;
mod finalization;
mod initialization;
mod observability;
mod revision10;
mod revision10_evidence;
mod seal_durability;
mod slot_contract;

use super::{run, FakePort, LifecycleError, Terminal};
use crate::{campaign_plan, CampaignCycle};

fn run1(fake: &mut FakePort) -> Result<Terminal, LifecycleError> {
    run(&campaign_plan(CampaignCycle::Run1), fake)
}
