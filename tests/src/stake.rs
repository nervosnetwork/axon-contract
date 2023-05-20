use std::collections::BTreeSet;
use std::convert::TryInto;

use crate::smt::{
    construct_epoch_smt, construct_lock_info_smt, u64_to_h256, TopSmtInfo, BOTTOM_SMT,
};

use super::*;
use axon_types::stake::*;
use bit_vec::BitVec;
use ckb_system_scripts::BUNDLED_CELL;
use ckb_testtool::ckb_crypto::secp::Generator;
use ckb_testtool::ckb_types::{bytes::Bytes, core::TransactionBuilder, packed::*, prelude::*};
use ckb_testtool::{builtin::ALWAYS_SUCCESS, context::Context};
use helper::*;
use molecule::prelude::*;
use util::smt::LockInfo;

#[test]
fn test_stake_at_increase_success() {
    // init context
    let mut context = Context::default();
    let secp256k1_data_bin = BUNDLED_CELL.get("specs/cells/secp256k1_data").unwrap();
    let secp256k1_data_out_point = context.deploy_cell(secp256k1_data_bin.to_vec().into());
    let secp256k1_data_dep = CellDep::new_builder()
        .out_point(secp256k1_data_out_point)
        .build();
    let contract_bin: Bytes = Loader::default().load_binary("stake");
    let contract_out_point = context.deploy_cell(contract_bin);
    let contract_dep = CellDep::new_builder()
        .out_point(contract_out_point.clone())
        .build();
    let always_success_out_point = context.deploy_cell(ALWAYS_SUCCESS.clone());
    let always_success_lock_script = context
        .build_script(&always_success_out_point, Bytes::from(vec![1]))
        .expect("always_success script");
    let checkpoint_type_script = context
        .build_script(&always_success_out_point, Bytes::from(vec![2]))
        .expect("checkpoint script");
    println!(
        "checkpoint type hash: {:?}",
        checkpoint_type_script.calc_script_hash().as_slice()
    );
    // let stake_at_lock_script = context
    //     .build_script(&always_success_out_point, Bytes::from(vec![3]))
    //     .expect("stake at script");
    let stake_at_type_script = context
        .build_script(&always_success_out_point, Bytes::from(vec![4]))
        .expect("sudt script");
    let metadata_type_script = context
        .build_script(&always_success_out_point, Bytes::from(vec![5]))
        .expect("metadata type script");
    let always_success_script_dep = CellDep::new_builder()
        .out_point(always_success_out_point.clone())
        .build();

    // prepare stake_args and stake_data
    let keypair = Generator::random_keypair();
    let stake_args = stake::StakeArgs::new_builder()
        .metadata_type_id(axon_byte32(&metadata_type_script.calc_script_hash()))
        .stake_addr(axon_identity_opt(&keypair.1.serialize()))
        .build();

    let input_stake_info_delta = stake::StakeInfoDelta::new_builder()
        .is_increase(1.into())
        .amount(axon_u128(0 as u128))
        .inauguration_epoch(axon_u64(0 as u64))
        .build();
    let input_stake_at_data = axon_stake_at_cell_data_without_amount(
        0,
        &keypair.1.serialize(),
        &keypair.1.serialize(),
        &metadata_type_script.calc_script_hash(),
        input_stake_info_delta,
    );

    // prepare stake lock_script
    let stake_at_lock_script = context
        .build_script(&contract_out_point, stake_args.as_bytes())
        .expect("stake script");

    // prepare tx inputs and outputs
    // println!("stake at cell lock hash:{:?}", stake_at_lock_script.calc_script_hash().as_slice());
    let inputs = vec![
        // stake AT cell
        CellInput::new_builder()
            .previous_output(
                context.create_cell(
                    CellOutput::new_builder()
                        .capacity(1000.pack())
                        .lock(stake_at_lock_script.clone())
                        .type_(Some(stake_at_type_script.clone()).pack())
                        .build(),
                    Bytes::from(axon_stake_at_cell_data(0, input_stake_at_data)),
                ),
            )
            .build(),
        // normal AT cell
        CellInput::new_builder()
            .previous_output(
                context.create_cell(
                    CellOutput::new_builder()
                        .capacity(1000.pack())
                        .lock(always_success_lock_script.clone())
                        .type_(Some(stake_at_type_script.clone()).pack())
                        .build(),
                    Bytes::from((1000 as u128).to_le_bytes().to_vec()),
                ),
            )
            .build(),
    ];
    let outputs = vec![
        // stake at cell
        CellOutput::new_builder()
            .capacity(1000.pack())
            .lock(stake_at_lock_script)
            .type_(Some(stake_at_type_script.clone()).pack())
            .build(),
        // stake cell
        CellOutput::new_builder()
            .capacity(1000.pack())
            .lock(always_success_lock_script.clone())
            .type_(Some(stake_at_type_script.clone()).pack())
            .build(),
    ];

    // prepare outputs_data
    let output_stake_info_delta = stake::StakeInfoDelta::new_builder()
        .is_increase(1.into())
        .amount(axon_u128(100 as u128))
        .inauguration_epoch(axon_u64(3 as u64))
        .build();
    let output_stake_at_data = axon_stake_at_cell_data_without_amount(
        0,
        &keypair.1.serialize(),
        &keypair.1.serialize(),
        &metadata_type_script.calc_script_hash(),
        output_stake_info_delta,
    );
    let outputs_data = vec![
        Bytes::from(axon_stake_at_cell_data(100, output_stake_at_data)), // stake at cell
        Bytes::from((900 as u128).to_le_bytes().to_vec()),               // normal at cell
                                                                         // Bytes::from(axon_withdrawal_data(50, 2)),
    ];

    // prepare metadata cell_dep
    let meta_data = axon_metadata_data(
        &metadata_type_script.clone().calc_script_hash(),
        &stake_at_type_script.calc_script_hash(),
        &checkpoint_type_script.calc_script_hash(),
        &stake_at_type_script.calc_script_hash(), // needless here
    );
    let metadata_script_dep = CellDep::new_builder()
        .out_point(
            context.create_cell(
                CellOutput::new_builder()
                    .capacity(1000.pack())
                    .lock(always_success_lock_script.clone())
                    .type_(Some(metadata_type_script.clone()).pack())
                    .build(),
                meta_data.as_bytes(),
            ),
        )
        .build();
    // prepare checkpoint cell_dep
    let checkpoint_data = axon_checkpoint_data(&metadata_type_script.clone().calc_script_hash());
    println!("checkpoint data: {:?}", checkpoint_data.as_bytes().len());
    let checkpoint_script_dep = CellDep::new_builder()
        .out_point(
            context.create_cell(
                CellOutput::new_builder()
                    .capacity(1000.pack())
                    .lock(always_success_lock_script.clone())
                    .type_(Some(checkpoint_type_script).pack())
                    .build(),
                checkpoint_data.as_bytes(),
            ),
        )
        .build();

    // prepare signed tx
    let tx = TransactionBuilder::default()
        .inputs(inputs)
        .outputs(outputs)
        .outputs_data(outputs_data.pack())
        .cell_dep(contract_dep)
        .cell_dep(always_success_script_dep)
        .cell_dep(secp256k1_data_dep)
        .cell_dep(checkpoint_script_dep)
        .cell_dep(metadata_script_dep)
        .build();
    let tx = context.complete_tx(tx);

    // sign tx for stake at cell (update stake at cell delta mode)
    let tx = sign_tx(tx, &keypair.0, 0);

    // run
    let cycles = context
        .verify_tx(&tx, MAX_CYCLES)
        .expect("pass verification");
    println!("consume cycles: {}", cycles);
}

#[test]
pub fn test_stake_withdraw_success() {
    println!("hello");

    // prepare withdraw lock_script
    // let withdrawal_args = axon_types::withdraw::WithdrawArgs::new_builder()
    //     .metadata_type_id(axon_byte32(&metadata_type_script.calc_script_hash()))
    //     .addr(axon_identity(&vec![0u8; 20]))
    //     .build();
    // let withdrawal_lock_script = Script::new_builder()
    //     .code_hash([0u8; 32].pack())
    //     .hash_type(ScriptHashType::Type.into())
    //     .args(withdrawal_args.as_slice().pack())
    //     .build();

    // withdrawal cell
    // CellOutput::new_builder()
    //     .capacity(1000.pack())
    //     .lock(withdrawal_lock_script.clone())
    //     .type_(Some(stake_at_type_script.clone()).pack())
    //     .build(),
}

#[test]
fn test_stake_smt_success() {
    // init context
    let mut context = Context::default();
    let secp256k1_data_bin = BUNDLED_CELL.get("specs/cells/secp256k1_data").unwrap();
    let secp256k1_data_out_point = context.deploy_cell(secp256k1_data_bin.to_vec().into());
    let secp256k1_data_dep = CellDep::new_builder()
        .out_point(secp256k1_data_out_point)
        .build();
    let contract_bin: Bytes = Loader::default().load_binary("stake");
    let contract_out_point = context.deploy_cell(contract_bin);
    let contract_dep = CellDep::new_builder()
        .out_point(contract_out_point.clone())
        .build();
    let always_success_out_point = context.deploy_cell(ALWAYS_SUCCESS.clone());
    let always_success_lock_script = context
        .build_script(&always_success_out_point, Bytes::from(vec![1]))
        .expect("always_success script");
    let checkpoint_type_script = context
        .build_script(&always_success_out_point, Bytes::from(vec![2]))
        .expect("checkpoint script");
    println!(
        "checkpoint type hash: {:?}",
        checkpoint_type_script.calc_script_hash().as_slice()
    );

    let stake_at_type_script = context
        .build_script(&always_success_out_point, Bytes::from(vec![4]))
        .expect("sudt script");
    let metadata_type_script = context
        .build_script(&always_success_out_point, Bytes::from(vec![5]))
        .expect("metadata type script");
    let always_success_script_dep = CellDep::new_builder()
        .out_point(always_success_out_point.clone())
        .build();

    // prepare stake_args and stake_data
    let keypair = Generator::random_keypair();
    let stake_args = stake::StakeArgs::new_builder()
        .metadata_type_id(axon_byte32(&metadata_type_script.calc_script_hash()))
        .stake_addr(axon_identity_opt(&keypair.1.serialize()))
        .build();

    let input_stake_info_delta = stake::StakeInfoDelta::new_builder()
        .is_increase(1.into())
        .amount(axon_u128(100 as u128))
        .inauguration_epoch(axon_u64(3 as u64))
        .build();
    let input_stake_at_data = axon_stake_at_cell_data_without_amount(
        0,
        &keypair.1.serialize(),
        &keypair.1.serialize(),
        &metadata_type_script.calc_script_hash(),
        input_stake_info_delta,
    );

    // prepare stake lock_script
    let stake_at_lock_script = context
        .build_script(&contract_out_point, stake_args.as_bytes())
        .expect("stake at lock script");
    let stake_smt_args = stake::StakeArgs::new_builder()
        .metadata_type_id(axon_byte32(&metadata_type_script.calc_script_hash()))
        .stake_addr(axon_identity_none())
        .build();
    let stake_smt_type_script = context
        .build_script(&contract_out_point, stake_smt_args.as_bytes())
        .expect("stake smt type script");

    // prepare tx inputs and outputs
    // println!("stake at cell lock hash:{:?}", stake_at_lock_script.calc_script_hash().as_slice());
    let input_stake_infos = BTreeSet::new();
    let input_stake_smt_data =
        axon_stake_smt_cell_data(&input_stake_infos, &metadata_type_script.calc_script_hash());

    let inputs = vec![
        // stake AT cell
        CellInput::new_builder()
            .previous_output(
                context.create_cell(
                    CellOutput::new_builder()
                        .capacity(1000.pack())
                        .lock(stake_at_lock_script.clone())
                        .type_(Some(stake_at_type_script.clone()).pack())
                        .build(),
                    Bytes::from(axon_stake_at_cell_data(100, input_stake_at_data)),
                ),
            )
            .build(),
        // stake smt cell
        CellInput::new_builder()
            .previous_output(
                context.create_cell(
                    CellOutput::new_builder()
                        .capacity(1000.pack())
                        .lock(always_success_lock_script.clone())
                        .type_(Some(stake_smt_type_script.clone()).pack())
                        .build(),
                    input_stake_smt_data.as_bytes(),
                ),
            )
            .build(),
    ];
    let outputs = vec![
        // stake at cell
        CellOutput::new_builder()
            .capacity(1000.pack())
            .lock(stake_at_lock_script)
            .type_(Some(stake_at_type_script.clone()).pack())
            .build(),
        // stake smt cell
        CellOutput::new_builder()
            .capacity(1000.pack())
            .lock(always_success_lock_script.clone())
            .type_(Some(stake_at_type_script.clone()).pack())
            .build(),
    ];

    // prepare outputs_data
    let output_stake_info_delta = stake::StakeInfoDelta::new_builder()
        .is_increase(1.into())
        .amount(axon_u128(0 as u128))
        .inauguration_epoch(axon_u64(0 as u64))
        .build();
    let output_stake_at_data = axon_stake_at_cell_data_without_amount(
        0,
        &keypair.1.serialize(),
        &keypair.1.serialize(),
        &metadata_type_script.calc_script_hash(),
        output_stake_info_delta,
    );

    let output_stake_infos = BTreeSet::new();
    let output_stake_smt_data = axon_stake_smt_cell_data(
        &output_stake_infos,
        &metadata_type_script.calc_script_hash(),
    );
    let outputs_data = vec![
        Bytes::from(axon_stake_at_cell_data(100, output_stake_at_data)), // stake at cell
        output_stake_smt_data.as_bytes(),                                // stake smt cell
    ];

    // prepare metadata cell_dep
    let meta_data = axon_metadata_data(
        &metadata_type_script.clone().calc_script_hash(),
        &stake_at_type_script.calc_script_hash(),
        &checkpoint_type_script.calc_script_hash(),
        &stake_smt_type_script.calc_script_hash(),
    );
    let metadata_script_dep = CellDep::new_builder()
        .out_point(
            context.create_cell(
                CellOutput::new_builder()
                    .capacity(1000.pack())
                    .lock(always_success_lock_script.clone())
                    .type_(Some(metadata_type_script.clone()).pack())
                    .build(),
                meta_data.as_bytes(),
            ),
        )
        .build();
    // prepare checkpoint cell_dep
    let checkpoint_data = axon_checkpoint_data(&metadata_type_script.clone().calc_script_hash());
    println!("checkpoint data: {:?}", checkpoint_data.as_bytes().len());
    let checkpoint_script_dep = CellDep::new_builder()
        .out_point(
            context.create_cell(
                CellOutput::new_builder()
                    .capacity(1000.pack())
                    .lock(always_success_lock_script.clone())
                    .type_(Some(checkpoint_type_script).pack())
                    .build(),
                checkpoint_data.as_bytes(),
            ),
        )
        .build();

    // construct old epoch proof
    let bottom_tree = BOTTOM_SMT::default();
    let old_bottom_root = bottom_tree.root();
    let top_smt_infos = vec![TopSmtInfo {
        epoch: 3,
        smt_root: *old_bottom_root,
    }];
    let (_, old_proof) = construct_epoch_smt(&top_smt_infos);
    let old_proof = old_proof.compile(vec![u64_to_h256(3)]).unwrap().0;

    let lock_info = LockInfo {
        addr: blake160(keypair.1.serialize().as_slice()),
        amount: 100,
    };
    let lock_infos = vec![lock_info].into_iter().collect::<BTreeSet<LockInfo>>();
    let (new_bottom_root, _) = construct_lock_info_smt(&lock_infos);
    let new_top_smt_infos = vec![TopSmtInfo {
        epoch: 3,
        smt_root: new_bottom_root,
    }];
    let (_, new_proof) = construct_epoch_smt(&new_top_smt_infos);
    let new_proof = new_proof.compile(vec![u64_to_h256(3)]).unwrap().0;

    let stake_info = stake::StakeInfo::new_builder()
        .addr(axon_identity(&keypair.1.serialize().as_slice().to_vec()))
        .amount(axon_u128(100))
        .build();
    let stake_infos = stake::StakeInfos::new_builder().push(stake_info).build();
    let stake_smt_update_info = stake::StakeSmtUpdateInfo::new_builder()
        .all_stake_infos(stake_infos)
        .old_epoch_proof(axon_bytes(&old_proof))
        .new_epoch_proof(axon_bytes(&new_proof))
        .build();

    let stake_smt_witness = WitnessArgs::new_builder()
        .lock(Some(Bytes::from(stake_smt_update_info.as_bytes())).pack())
        .input_type(Some(Bytes::from(vec![1])).pack())
        .build();
    let stake_at_witness = WitnessArgs::new_builder()
        .input_type(Some(Bytes::from(vec![1])).pack())
        .build();

    // prepare signed tx
    let tx = TransactionBuilder::default()
        .inputs(inputs)
        .outputs(outputs)
        .outputs_data(outputs_data.pack())
        .witnesses(vec![
            stake_at_witness.as_bytes().pack(),
            stake_smt_witness.as_bytes().pack(),
        ])
        .cell_dep(contract_dep)
        .cell_dep(always_success_script_dep)
        .cell_dep(secp256k1_data_dep)
        .cell_dep(checkpoint_script_dep)
        .cell_dep(metadata_script_dep)
        .build();
    let tx = context.complete_tx(tx);

    // sign tx for stake at cell (update stake smt cell)
    // let tx = sign_tx(tx, &keypair.0, 1);

    // run
    let cycles = context
        .verify_tx(&tx, MAX_CYCLES)
        .expect("pass verification");
    println!("consume cycles: {}", cycles);
}
