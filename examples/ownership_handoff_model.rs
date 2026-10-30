//! Synthetic, in-memory ownership MODEL demonstration; no Store or real grant.
use quipu::ownership_handoff_model::{
    Action, Authority, AuthorityUnit, Certificate, Cut, Model, Observations, Placement, Request,
};

fn main() {
    let a = Authority {
        store_id: "synthetic-lineage".into(),
        unit: AuthorityUnit::FullStore,
        generation: 0,
        epoch: "synthetic-A-epoch".into(),
        owner: Placement {
            principal: "A".into(),
            native: [1; 32],
        },
        policy: "assumed-existing-policy".into(),
        predecessor: None,
    };
    let c = Certificate {
        request: Request {
            operation: "synthetic-operation".into(),
            operator: "assumed-existing-operator".into(),
            source: a.clone(),
            destination: Placement {
                principal: "B".into(),
                native: [2; 32],
            },
            successor_generation: 1,
            profile: "assumed-existing-profile".into(),
        },
        cut: Cut {
            transaction: 7,
            facts: [3; 32],
            artifact: [4; 32],
        },
    };
    let observations = Observations {
        authenticated: true,
        current_grants: true,
        witness_online: true,
    };
    let mut model = Model::new(a.clone()).unwrap();
    model
        .apply(&Action::Prepare(c.request.clone()), observations)
        .unwrap();
    println!("MODEL only; oracle assumptions are not credentials");
    println!(
        "before_retirement old_A_can_commit={}",
        model.can_commit(&a)
    );
    let early = model
        .apply(&Action::Publish(c.clone()), observations)
        .is_ok();
    println!(
        "premature_receipt={}",
        if early { "PUBLISHED" } else { "REFUSED" }
    );
    if !early {
        for action in [
            Action::Barrier(c.request.clone()),
            Action::Seal(c.clone()),
            Action::Retire(c.clone()),
            Action::Publish(c.clone()),
        ] {
            model.apply(&action, observations).unwrap();
        }
        println!("after_retirement old_A_can_commit={}", model.can_commit(&a));
    } else {
        println!("source_barrier=NOT_EXECUTED");
    }
    let epoch = "synthetic-B-epoch";
    for action in [
        Action::Pending {
            certificate: c.clone(),
            epoch: epoch.into(),
            artifact: c.cut.artifact,
        },
        Action::Install {
            certificate: c.clone(),
            artifact: c.cut.artifact,
        },
        Action::AdvanceWitness(c.clone()),
        Action::Activate {
            certificate: c.clone(),
            epoch: epoch.into(),
            nonce: "fresh-B".into(),
            challenge: "fresh-B".into(),
        },
    ] {
        model.apply(&action, observations).unwrap();
    }
    let b = c.successor(epoch).unwrap();
    println!(
        "after_B_admission old_A_can_commit={} B_can_commit={} admitted_owners={}",
        model.can_commit(&a),
        model.can_commit(&b),
        model.writable().len()
    );
    assert!(
        !early && !model.can_commit(&a) && model.can_commit(&b) && model.writable().len() == 1,
        "retirement prerequisite lost"
    );
}
