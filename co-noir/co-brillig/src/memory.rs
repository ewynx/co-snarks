use acvm::brillig_vm::MemoryValue;
use ark_ff::PrimeField;
use brillig::MemoryAddress;

use crate::mpc::BrilligDriver;

/**
*  Copied form https://github.com/noir-lang/noir/blob/68c32b4ffd9b069fe4b119327dbf4018c17ab9d4/acvm-repo/brillig_vm/src/memory.rs
*
*  We cannot use the implementation because it is bound to [AcirField]
**/

pub(super) struct Memory<T, F>
where
    T: BrilligDriver<F>,
    F: PrimeField,
{
    inner: Vec<MemoryValue<T::BrilligType>>,
}

impl<T, F> Memory<T, F>
where
    T: BrilligDriver<F>,
    F: PrimeField,
{
    pub(super) fn new() -> Self {
        Self { inner: vec![] }
    }

    fn get_stack_pointer(&self) -> usize {
        self.read(MemoryAddress::Direct(0)).to_usize()
    }

    fn resolve(&self, address: MemoryAddress) -> usize {
        match address {
            MemoryAddress::Direct(address) => address,
            MemoryAddress::Relative(offset) => self.get_stack_pointer() + offset,
        }
    }

    /// Gets the value at address
    pub fn read(&self, address: MemoryAddress) -> MemoryValue<T::BrilligType> {
        let resolved_addr = self.resolve(address);
        if let Some(val) = self.inner.get(resolved_addr) {
            val.clone()
        } else {
            MemoryValue::new_field(T::BrilligType::default())
        }
    }

    pub fn read_ref(&self, ptr: MemoryAddress) -> MemoryAddress {
        MemoryAddress::direct(self.read(ptr).to_usize())
    }

    pub fn read_slice(&self, addr: MemoryAddress, len: usize) -> &[MemoryValue<T::BrilligType>] {
        // Allows to read a slice of uninitialized memory if the length is zero.
        // Ideally we'd be able to read uninitialized memory in general (as read does)
        // but that's not possible if we want to return a slice instead of owned data.
        if len == 0 {
            return &[];
        }
        let resolved_addr = self.resolve(addr);
        &self.inner[resolved_addr..(resolved_addr + len)]
    }

    /// Sets the value at `address` to `value`
    pub fn write(&mut self, address: MemoryAddress, value: MemoryValue<T::BrilligType>) {
        let resolved_ptr = self.resolve(address);
        self.resize_to_fit(resolved_ptr + 1);
        self.inner[resolved_ptr] = value;
    }

    fn resize_to_fit(&mut self, size: usize) {
        // Calculate new memory size
        let new_size = std::cmp::max(self.inner.len(), size);
        // Expand memory to new size with default values if needed
        self.inner
            .resize(new_size, MemoryValue::new_field(T::BrilligType::default()));
    }

    /// Sets the values after `address` to `values`
    pub fn write_slice(&mut self, address: MemoryAddress, values: &[MemoryValue<T::BrilligType>]) {
        let resolved_address = self.resolve(address);
        self.resize_to_fit(resolved_address + values.len());
        self.inner[resolved_address..(resolved_address + values.len())].clone_from_slice(values);
    }

    /// Returns the values of the memory
    pub fn values(&self) -> &[MemoryValue<T::BrilligType>] {
        &self.inner
    }
}

// we paste here some methods copied from Brillig Repo. Unfortunately, we cannot
// call a lot of function because they are generic over AcirField, therefore we need to
// copy them here
pub(super) mod memory_utils {
    use acvm::brillig_vm::MemoryValue;
    use brillig::IntegerBitSize;

    pub fn expect_int_with_bit_size<F>(
        value: MemoryValue<F>,
        expected_bit_size: IntegerBitSize,
    ) -> eyre::Result<u128> {
        match value {
            MemoryValue::Integer(value, bit_size) => {
                if bit_size != expected_bit_size {
                    eyre::bail!(
                        "expected bit size {}, but is {}",
                        expected_bit_size,
                        bit_size
                    )
                }
                Ok(value)
            }
            MemoryValue::Field(_) => eyre::bail!("expected int but got Field"),
        }
    }

    pub fn to_bool<F>(value: MemoryValue<F>) -> eyre::Result<bool> {
        let bool_val = expect_int_with_bit_size(value, IntegerBitSize::U1)?;
        Ok(bool_val != 0)
    }
}