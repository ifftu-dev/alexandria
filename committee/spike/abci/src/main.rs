use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tendermint_abci::{Application, ServerBuilder};
use tendermint_proto::v0_38::abci::{
    response_process_proposal, ExecTxResult, RequestCheckTx, RequestFinalizeBlock, RequestInfo,
    RequestInitChain, RequestProcessProposal, RequestQuery, ResponseCheckTx, ResponseCommit,
    ResponseFinalizeBlock, ResponseInfo, ResponseInitChain, ResponseProcessProposal, ResponseQuery,
};

const STATE_DOMAIN: &[u8] = b"alexandria/g01-state/v1";
const VALID_TX: &[u8] = b"increment";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CommittedState {
    height: i64,
    value: u64,
}

impl CommittedState {
    fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(STATE_DOMAIN.len() + 16);
        bytes.extend_from_slice(STATE_DOMAIN);
        bytes.extend_from_slice(&self.height.to_be_bytes());
        bytes.extend_from_slice(&self.value.to_be_bytes());
        bytes
    }

    fn hash(&self) -> Vec<u8> {
        Sha256::digest(self.canonical_bytes()).to_vec()
    }
}

#[derive(Debug)]
struct RuntimeState {
    committed: CommittedState,
    pending: Option<CommittedState>,
}

#[derive(Clone)]
struct SpikeApp {
    state: Arc<Mutex<RuntimeState>>,
    state_path: Arc<PathBuf>,
}

impl SpikeApp {
    fn open(state_path: PathBuf) -> Result<Self, String> {
        let committed = if state_path.exists() {
            let bytes = fs::read(&state_path).map_err(|error| error.to_string())?;
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())?
        } else {
            CommittedState::default()
        };
        Ok(Self {
            state: Arc::new(Mutex::new(RuntimeState {
                committed,
                pending: None,
            })),
            state_path: Arc::new(state_path),
        })
    }

    fn persist(path: &Path, state: &CommittedState) -> Result<(), String> {
        let bytes = serde_json::to_vec(state).map_err(|error| error.to_string())?;
        let temp_path = path.with_extension("tmp");
        fs::write(&temp_path, bytes).map_err(|error| error.to_string())?;
        fs::rename(&temp_path, path).map_err(|error| error.to_string())
    }

    fn tx_result(code: u32, log: &str) -> ExecTxResult {
        ExecTxResult {
            code,
            log: log.to_owned(),
            ..Default::default()
        }
    }
}

impl Application for SpikeApp {
    fn info(&self, request: RequestInfo) -> ResponseInfo {
        if request.abci_version != "2.0.0" {
            eprintln!(
                "unexpected ABCI version from daemon: {}",
                request.abci_version
            );
        }
        let state = self.state.lock().expect("state mutex poisoned");
        ResponseInfo {
            data: "alexandria-g01-spike".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            app_version: 1,
            last_block_height: state.committed.height,
            last_block_app_hash: state.committed.hash().into(),
        }
    }

    fn init_chain(&self, request: RequestInitChain) -> ResponseInitChain {
        eprintln!(
            "ABCI_INIT_CHAIN chain_id={} initial_height={}",
            request.chain_id, request.initial_height
        );
        ResponseInitChain::default()
    }

    fn check_tx(&self, request: RequestCheckTx) -> ResponseCheckTx {
        let (code, log) = if request.tx.as_ref() == VALID_TX {
            (0, "accepted")
        } else {
            (1, "unknown transaction")
        };
        ResponseCheckTx {
            code,
            log: log.to_owned(),
            ..Default::default()
        }
    }

    fn process_proposal(&self, request: RequestProcessProposal) -> ResponseProcessProposal {
        let status = if request.txs.iter().all(|tx| tx.as_ref() == VALID_TX) {
            response_process_proposal::ProposalStatus::Accept
        } else {
            response_process_proposal::ProposalStatus::Reject
        };
        ResponseProcessProposal {
            status: status as i32,
        }
    }

    fn finalize_block(&self, request: RequestFinalizeBlock) -> ResponseFinalizeBlock {
        let mut runtime = self.state.lock().expect("state mutex poisoned");
        let expected_height = runtime.committed.height + 1;
        if request.height != expected_height {
            return ResponseFinalizeBlock {
                tx_results: request
                    .txs
                    .iter()
                    .map(|_| Self::tx_result(2, "unexpected height"))
                    .collect(),
                app_hash: runtime.committed.hash().into(),
                ..Default::default()
            };
        }

        let mut next = runtime.committed.clone();
        next.height = request.height;
        let mut tx_results = Vec::with_capacity(request.txs.len());
        for tx in request.txs {
            if tx.as_ref() == VALID_TX {
                next.value = next.value.checked_add(1).expect("spike counter overflow");
                tx_results.push(Self::tx_result(0, "accepted"));
            } else {
                tx_results.push(Self::tx_result(1, "unknown transaction"));
            }
        }

        let app_hash = next.hash();
        runtime.pending = Some(next);
        ResponseFinalizeBlock {
            tx_results,
            app_hash: app_hash.into(),
            ..Default::default()
        }
    }

    fn commit(&self) -> ResponseCommit {
        let mut runtime = self.state.lock().expect("state mutex poisoned");
        let pending = runtime
            .pending
            .take()
            .expect("CometBFT called Commit without FinalizeBlock");
        Self::persist(&self.state_path, &pending).expect("persist committed spike state");
        runtime.committed = pending;
        ResponseCommit { retain_height: 0 }
    }

    fn query(&self, request: RequestQuery) -> ResponseQuery {
        if request.path != "/state" || request.data.as_ref() != b"state" {
            return ResponseQuery {
                code: 1,
                log: "unknown query".to_owned(),
                ..Default::default()
            };
        }
        let runtime = self.state.lock().expect("state mutex poisoned");
        ResponseQuery {
            code: 0,
            key: b"state".to_vec().into(),
            value: runtime.committed.canonical_bytes().into(),
            height: runtime.committed.height,
            ..Default::default()
        }
    }
}

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("serve") => {
            let listen = args.next().unwrap_or_else(|| "127.0.0.1:26658".to_owned());
            let path = args
                .next()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("state.json"));
            let app = SpikeApp::open(path)?;
            let server = ServerBuilder::default()
                .bind(&listen, app)
                .map_err(|error| format!("bind ABCI server: {error}"))?;
            eprintln!("ABCI_LISTEN={}", server.local_addr());
            server.listen().map_err(|error| error.to_string())
        }
        Some("verify-state") => {
            let height = args
                .next()
                .ok_or("missing height")?
                .parse::<i64>()
                .map_err(|error| error.to_string())?;
            let value = args
                .next()
                .ok_or("missing value")?
                .parse::<u64>()
                .map_err(|error| error.to_string())?;
            let expected = args.next().ok_or("missing expected hash")?;
            let actual = hex::encode_upper(CommittedState { height, value }.hash());
            if actual != expected.to_uppercase() {
                return Err(format!("state hash mismatch: expected {expected}, got {actual}"));
            }
            println!("verified state height={height} value={value} hash={actual}");
            Ok(())
        }
        _ => Err("usage: alexandria-g01-spike serve [listen] [state-path] | verify-state <height> <value> <hex-hash>".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::CommittedState;

    #[test]
    fn committed_state_matches_the_daemon_fixture() {
        let hash = CommittedState {
            height: 15,
            value: 1,
        }
        .hash();

        assert_eq!(
            hex::encode_upper(hash),
            "D1135C44CB26A136D9D036B3451C7DBA718BF968427D846E26BFB5DC87CFDC53"
        );
    }
}
