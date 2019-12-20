//! Implementation for a contract method builder and call future. This is not
//! intended to be used directly but to be used by a contract `Instance` with
//! [Instance::method](ethcontract::contract::Instance::method).

use crate::errors::ExecutionError;
use crate::future::CompatQueryResult;
use crate::transaction::{Account, SendAndConfirmFuture, SendFuture, TransactionBuilder};
use crate::truffle::abi::Function;
use futures::compat::Future01CompatExt;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use web3::api::Web3;
use web3::contract::tokens::Detokenize;
use web3::contract::QueryResult;
use web3::types::{Address, BlockNumber, Bytes, CallRequest, U256};
use web3::Transport;

/// Default options to be applied to `MethodBuilder` or `ViewMethodBuilder`.
#[derive(Clone, Debug, Default)]
pub struct MethodDefaults {
    /// Default sender of the transaction with the signing strategy to use.
    pub from: Option<Account>,
    /// Default gas amount to use for transaction.
    pub gas: Option<U256>,
    /// Default gas price to use for transaction.
    pub gas_price: Option<U256>,
}

/// Data used for building a contract method call or transaction. The method
/// builder can be demoted into a `CallBuilder` to not allow sending of
/// transactions. This is useful when dealing with view functions.
#[derive(Debug, Clone)]
#[must_use = "methods do nothing unless you `.call()` or `.send()` them"]
pub struct MethodBuilder<T: Transport, R> {
    web3: Web3<T>,
    function: Function,
    /// transaction parameters
    pub tx: TransactionBuilder<T>,
    _result: PhantomData<R>,
}

impl<T: Transport, R> MethodBuilder<T, R> {
    /// Creates a new builder for a transaction.
    pub fn new(
        web3: Web3<T>,
        function: Function,
        address: Address,
        data: Bytes,
    ) -> MethodBuilder<T, R> {
        MethodBuilder {
            web3: web3.clone(),
            function,
            tx: TransactionBuilder::new(web3).to(address).data(data),
            _result: PhantomData,
        }
    }

    /// Apply method defaults to this builder.
    pub fn with_defaults(mut self, defaults: &MethodDefaults) -> MethodBuilder<T, R> {
        self.tx.from = self.tx.from.or_else(|| defaults.from.clone());
        self.tx.gas = self.tx.gas.or(defaults.gas);
        self.tx.gas_price = self.tx.gas_price.or(defaults.gas_price);
        self
    }

    /// Specify the signing method to use for the transaction, if not specified
    /// the the transaction will be locally signed with the default user.
    pub fn from(mut self, value: Account) -> MethodBuilder<T, R> {
        self.tx = self.tx.from(value);
        self
    }

    /// Secify amount of gas to use, if not specified then a gas estimate will
    /// be used.
    pub fn gas(mut self, value: U256) -> MethodBuilder<T, R> {
        self.tx = self.tx.gas(value);
        self
    }

    /// Specify the gas price to use, if not specified then the estimated gas
    /// price will be used.
    pub fn gas_price(mut self, value: U256) -> MethodBuilder<T, R> {
        self.tx = self.tx.gas_price(value);
        self
    }

    /// Specify what how much ETH to transfer with the transaction, if not
    /// specified then no ETH will be sent.
    pub fn value(mut self, value: U256) -> MethodBuilder<T, R> {
        self.tx = self.tx.value(value);
        self
    }

    /// Specify the nonce for the transation, if not specified will use the
    /// current transaction count for the signing account.
    pub fn nonce(mut self, value: U256) -> MethodBuilder<T, R> {
        self.tx = self.tx.nonce(value);
        self
    }

    /// Extract inner `TransactionBuilder` from this `SendBuilder`. This exposes
    /// `TransactionBuilder` only APIs.
    pub fn into_inner(self) -> TransactionBuilder<T> {
        self.tx
    }

    /// Sign (if required) and send the method call transaction.
    pub fn send(self) -> SendFuture<T> {
        self.tx.send()
    }

    /// Send a transaction for the method call and wait for confirmation.
    /// Returns the transaction receipt for inspection.
    pub fn send_and_confirm(
        self,
        poll_interval: Duration,
        confirmations: usize,
    ) -> SendAndConfirmFuture<T> {
        self.tx.send_and_confirm(poll_interval, confirmations)
    }
}

impl<T: Transport, R: Detokenize> MethodBuilder<T, R> {
    /// Demotes a `MethodBuilder` into a `ViewMethodBuilder` which has a more
    /// restricted API and cannot actually send transactions.
    pub fn view(self) -> ViewMethodBuilder<T, R> {
        ViewMethodBuilder::from_method(self)
    }

    /// Call a contract method. Contract calls do not modify the blockchain and
    /// as such do not require gas or signing. Note that doing a call with a
    /// block number requires first demoting the `MethodBuilder` into a
    /// `ViewMethodBuilder` and setting the block number for the call.
    pub fn call(self) -> CallFuture<T, R> {
        self.view().call()
    }
}

/// Data used for building a contract method call. The view method builder can't
/// directly send transactions and is for read only method calls.
#[derive(Debug, Clone)]
#[must_use = "view methods do nothing unless you `.call()` them"]
pub struct ViewMethodBuilder<T: Transport, R: Detokenize> {
    /// method parameters
    pub m: MethodBuilder<T, R>,
    /// optional block number
    pub block: Option<BlockNumber>,
}

impl<T: Transport, R: Detokenize> ViewMethodBuilder<T, R> {
    /// Create a new `ViewMethodBuilder` by demoting a `MethodBuilder`.
    pub fn from_method(method: MethodBuilder<T, R>) -> ViewMethodBuilder<T, R> {
        ViewMethodBuilder {
            m: method,
            block: None,
        }
    }

    /// Apply method defaults to this builder.
    pub fn with_defaults(mut self, defaults: &MethodDefaults) -> ViewMethodBuilder<T, R> {
        self.m = self.m.with_defaults(defaults);
        self
    }

    /// Specify the account the transaction is being sent from.
    pub fn from(mut self, value: Address) -> ViewMethodBuilder<T, R> {
        self.m = self.m.from(Account::Local(value, None));
        self
    }

    /// Secify amount of gas to use, if not specified then a gas estimate will
    /// be used.
    pub fn gas(mut self, value: U256) -> ViewMethodBuilder<T, R> {
        self.m = self.m.gas(value);
        self
    }

    /// Specify the gas price to use, if not specified then the estimated gas
    /// price will be used.
    pub fn gas_price(mut self, value: U256) -> ViewMethodBuilder<T, R> {
        self.m = self.m.gas_price(value);
        self
    }

    /// Specify what how much ETH to transfer with the transaction, if not
    /// specified then no ETH will be sent.
    pub fn value(mut self, value: U256) -> ViewMethodBuilder<T, R> {
        self.m = self.m.value(value);
        self
    }

    /// Specify the nonce for the transation, if not specified will use the
    /// current transaction count for the signing account.
    pub fn block(mut self, value: BlockNumber) -> ViewMethodBuilder<T, R> {
        self.block = Some(value);
        self
    }

    /// Call a contract method. Contract calls do not modify the blockchain and
    /// as such do not require gas or signing.
    pub fn call(self) -> CallFuture<T, R> {
        CallFuture::from_builder(self)
    }
}

/// Future representing a pending contract call (i.e. query) to be resolved when
/// the call completes.
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct CallFuture<T: Transport, R: Detokenize>(CompatQueryResult<T, R>);

impl<T: Transport, R: Detokenize> CallFuture<T, R> {
    /// Construct a new `CallFuture` from a `CallBuilder`.
    fn from_builder(builder: ViewMethodBuilder<T, R>) -> CallFuture<T, R> {
        CallFuture(
            QueryResult::new(
                builder.m.web3.eth().call(
                    CallRequest {
                        from: builder.m.tx.from.map(|account| account.address()),
                        to: builder.m.tx.to.unwrap_or_default(),
                        gas: builder.m.tx.gas,
                        gas_price: builder.m.tx.gas_price,
                        value: builder.m.tx.value,
                        data: builder.m.tx.data,
                    },
                    builder.block,
                ),
                builder.m.function,
            )
            .compat(),
        )
    }

    /// Get a pinned reference to the inner `QueryResult` web3 future taht is
    /// actually driving the query.
    fn inner(self: Pin<&mut Self>) -> Pin<&mut CompatQueryResult<T, R>> {
        Pin::new(&mut self.get_mut().0)
    }
}

impl<T: Transport, R: Detokenize> Future for CallFuture<T, R> {
    type Output = Result<R, ExecutionError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        self.inner()
            .poll(cx)
            .map(|result| result.map_err(ExecutionError::from))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::prelude::*;
    use crate::truffle::abi::{Param, ParamType};

    fn test_abi_function() -> (Function, Bytes) {
        let function = Function {
            name: "test".to_owned(),
            inputs: Vec::new(),
            outputs: vec![Param {
                name: "".to_owned(),
                kind: ParamType::Uint(256),
            }],
            constant: false,
        };
        let data = function
            .encode_input(&[])
            .expect("error encoding empty input");

        (function, Bytes(data))
    }

    #[test]
    fn method_tx_options() {
        let transport = TestTransport::new();
        let web3 = Web3::new(transport.clone());

        let address = addr!("0x0123456789012345678901234567890123456789");
        let from = addr!("0x9876543210987654321098765432109876543210");
        let (function, data) = test_abi_function();
        let tx = MethodBuilder::<_, U256>::new(web3, function, address, data.clone())
            .from(Account::Local(from, None))
            .gas(1.into())
            .gas_price(2.into())
            .value(28.into())
            .nonce(42.into())
            .into_inner();

        assert_eq!(tx.from.map(|a| a.address()), Some(from));
        assert_eq!(tx.to, Some(address));
        assert_eq!(tx.gas, Some(1.into()));
        assert_eq!(tx.gas_price, Some(2.into()));
        assert_eq!(tx.value, Some(28.into()));
        assert_eq!(tx.data, Some(data));
        assert_eq!(tx.nonce, Some(42.into()));
        transport.assert_no_more_requests();
    }

    #[test]
    fn view_method_call() {
        let mut transport = TestTransport::new();
        let web3 = Web3::new(transport.clone());

        let address = addr!("0x0123456789012345678901234567890123456789");
        let from = addr!("0x9876543210987654321098765432109876543210");
        let (function, data) = test_abi_function();
        let tx = ViewMethodBuilder::<_, U256>::from_method(MethodBuilder::new(
            web3,
            function,
            address,
            data.clone(),
        ))
        .from(from)
        .gas(1.into())
        .gas_price(2.into())
        .value(28.into())
        .block(BlockNumber::Number(100));

        transport.add_response(json!(
            "0x000000000000000000000000000000000000000000000000000000000000002a"
        )); // call response
        let result = tx.call().wait().expect("call error");

        assert_eq!(result, 42.into());
        transport.assert_request(
            "eth_call",
            &[
                json!({
                    "from": from,
                    "to": address,
                    "gas": "0x1",
                    "gasPrice": "0x2",
                    "value": "0x1c",
                    "data": data,
                }),
                json!("0x64"),
            ],
        );
        transport.assert_no_more_requests();
    }

    #[test]
    fn method_to_view_method_preserves_options() {
        let mut transport = TestTransport::new();
        let web3 = Web3::new(transport.clone());

        let address = addr!("0x0123456789012345678901234567890123456789");
        let (function, data) = test_abi_function();
        let tx = MethodBuilder::<_, U256>::new(web3, function, address, data.clone())
            .gas(42.into())
            .view();

        transport.add_response(json!(
            "0x0000000000000000000000000000000000000000000000000000000000000000"
        ));
        tx.call().wait().expect("call error");

        transport.assert_request(
            "eth_call",
            &[
                json!({
                    "to": address,
                    "gas": "0x2a",
                    "data": data,
                }),
                json!("latest"),
            ],
        );
        transport.assert_no_more_requests();
    }

    #[test]
    fn method_defaults_are_applied() {
        let transport = TestTransport::new();
        let web3 = Web3::new(transport.clone());

        let from = addr!("0x9876543210987654321098765432109876543210");
        let address = addr!("0x0123456789012345678901234567890123456789");
        let (function, data) = test_abi_function();
        let tx = MethodBuilder::<_, U256>::new(web3, function, address, data)
            .with_defaults(&MethodDefaults {
                from: Some(Account::Local(from, None)),
                gas: Some(1.into()),
                gas_price: Some(2.into()),
            })
            .into_inner();

        assert_eq!(tx.from.map(|a| a.address()), Some(from));
        assert_eq!(tx.gas, Some(1.into()));
        assert_eq!(tx.gas_price, Some(2.into()));
        transport.assert_no_more_requests();
    }
}