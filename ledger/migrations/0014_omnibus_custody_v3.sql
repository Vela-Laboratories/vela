-- V3 is the first omnibus-custody ABI. Customer beneficial ownership stays
-- in ledger_accounts; the contract never receives a customer balance identity.
ALTER TABLE ledger_withdrawal_authorizations
DROP CONSTRAINT ledger_withdrawal_authorizations_envelope_version_check;

ALTER TABLE ledger_withdrawal_authorizations
ADD CONSTRAINT ledger_withdrawal_authorizations_envelope_version_check CHECK (
    envelope_version IN (
        'vela.withdrawal.authorization.v1',
        'vela.withdrawal.authorization.eip712.v2',
        'vela.withdrawal.authorization.eip712.v3'
    )
);

ALTER TABLE ledger_withdrawal_authorizations
DROP CONSTRAINT ledger_withdrawal_authorizations_v2_hashes;

ALTER TABLE ledger_withdrawal_authorizations
ADD CONSTRAINT ledger_withdrawal_authorizations_eip712_hashes CHECK (
    (envelope_version = 'vela.withdrawal.authorization.v1'
        AND domain_separator IS NULL AND struct_hash IS NULL
        AND policy_hash IS NULL AND approver_hash IS NULL)
    OR
    (envelope_version IN (
            'vela.withdrawal.authorization.eip712.v2',
            'vela.withdrawal.authorization.eip712.v3'
        )
        AND domain_separator IS NOT NULL AND struct_hash IS NOT NULL
        AND policy_hash IS NOT NULL AND approver_hash IS NOT NULL)
);

ALTER TABLE ledger_withdrawal_contract_capabilities
DROP CONSTRAINT ledger_withdrawal_contract_capabili_authorization_version_check;

ALTER TABLE ledger_withdrawal_contract_capabilities
ADD CONSTRAINT ledger_withdrawal_contract_capabilities_authorization_version_check CHECK (
    authorization_version IN ('eip712_v2', 'eip712_v3')
);

ALTER TABLE ledger_withdrawal_contract_capabilities
DROP CONSTRAINT ledger_withdrawal_contract_capa_execution_event_signature_check;

ALTER TABLE ledger_withdrawal_contract_capabilities
ADD CONSTRAINT ledger_withdrawal_contract_capabilities_execution_event_signature_check CHECK (
    execution_event_signature IN (
        'WithdrawalExecuted(bytes16,address,address,address,uint256,uint256,uint256,bytes32,bytes32)',
        'WithdrawalExecuted(bytes16,address,address,uint256,uint256,uint256,bytes32,bytes32)'
    )
);

-- Executable authorization must use the ABI explicitly registered for the
-- immutable deployment. V2 evidence stays valid for historical deployments;
-- V3 is the omnibus custody ABI and intentionally carries no customer address.
CREATE OR REPLACE FUNCTION ledger_validate_executable_withdrawal_authorization()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM ledger_chain_deployments deployment
        JOIN ledger_withdrawal_contract_capabilities capability
          ON capability.deployment_id = deployment.id
        WHERE deployment.enabled
          AND deployment.chain_id = NEW.chain_id
          AND lower(deployment.vault_address) = lower(NEW.vault_address)
          AND (
              (NEW.envelope_version = 'vela.withdrawal.authorization.eip712.v2'
                  AND capability.authorization_version = 'eip712_v2')
              OR
              (NEW.envelope_version = 'vela.withdrawal.authorization.eip712.v3'
                  AND capability.authorization_version = 'eip712_v3')
          )
    ) THEN
        RAISE EXCEPTION 'withdrawal authorization requires a registered deployment with a matching EIP-712 ABI';
    END IF;
    RETURN NEW;
END;
$$;

-- Keep historical V2 evidence valid, but bind future observations to a
-- capability whose authorization ABI matches the persisted authorization.
CREATE OR REPLACE FUNCTION ledger_validate_withdrawal_chain_observation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    withdrawal_row ledger_withdrawals%ROWTYPE;
    authorization_row ledger_withdrawal_authorizations%ROWTYPE;
    deployment_row ledger_chain_deployments%ROWTYPE;
    capability_row ledger_withdrawal_contract_capabilities%ROWTYPE;
    expected_asset_address TEXT;
    required_confirmations BIGINT;
BEGIN
    SELECT * INTO withdrawal_row FROM ledger_withdrawals WHERE id = NEW.withdrawal_id FOR SHARE;
    SELECT * INTO authorization_row FROM ledger_withdrawal_authorizations
    WHERE withdrawal_id = NEW.withdrawal_id FOR SHARE;
    SELECT * INTO deployment_row FROM ledger_chain_deployments
    WHERE id = NEW.deployment_id AND enabled FOR SHARE;
    SELECT * INTO capability_row FROM ledger_withdrawal_contract_capabilities
    WHERE deployment_id = NEW.deployment_id FOR SHARE;
    SELECT COALESCE(contract_address, '0x0000000000000000000000000000000000000000')
    INTO expected_asset_address FROM ledger_assets WHERE id = withdrawal_row.asset_id;

    IF withdrawal_row.id IS NULL OR authorization_row.id IS NULL
       OR deployment_row.id IS NULL OR capability_row.deployment_id IS NULL
       OR NOT (
           (authorization_row.envelope_version = 'vela.withdrawal.authorization.eip712.v2'
                AND capability_row.authorization_version = 'eip712_v2')
           OR
           (authorization_row.envelope_version = 'vela.withdrawal.authorization.eip712.v3'
                AND capability_row.authorization_version = 'eip712_v3')
       )
       OR NEW.request_id <> withdrawal_row.id
       OR NEW.chain_id <> withdrawal_row.chain_id
       OR deployment_row.chain_id <> withdrawal_row.chain_id
       OR lower(NEW.vault_address) <> lower(withdrawal_row.vault_address)
       OR lower(deployment_row.vault_address) <> lower(withdrawal_row.vault_address)
       OR lower(NEW.destination_address) <> lower(withdrawal_row.destination_address)
       OR lower(NEW.asset_address) <> lower(expected_asset_address)
       OR NEW.amount <> withdrawal_row.amount
       OR NEW.withdrawal_nonce IS DISTINCT FROM withdrawal_row.nonce THEN
        RAISE EXCEPTION 'withdrawal chain observation does not match executable authorization facts';
    END IF;

    IF NEW.observation_type IN ('finalized_execution', 'finalized_revert', 'finalized_non_execution') THEN
        SELECT min_confirmations INTO required_confirmations
        FROM ledger_withdrawal_confirmation_policies
        WHERE deployment_id = NEW.deployment_id
          AND policy_version = NEW.confirmation_policy_version
          AND effective_at <= NEW.block_timestamp;
        IF required_confirmations IS NULL OR NEW.confirmations < required_confirmations THEN
            RAISE EXCEPTION 'withdrawal chain observation does not satisfy confirmation policy';
        END IF;
    END IF;
    IF NEW.observation_type IN ('finalized_execution', 'finalized_revert')
       AND NEW.transaction_hash IS NULL THEN
        RAISE EXCEPTION 'finalized withdrawal transaction evidence requires a transaction hash';
    END IF;
    IF NEW.observation_type = 'finalized_non_execution'
       AND NEW.block_timestamp < authorization_row.expires_at THEN
        RAISE EXCEPTION 'withdrawal non-execution cannot be final before authorization expiry';
    END IF;
    RETURN NEW;
END;
$$;
