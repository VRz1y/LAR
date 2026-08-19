//! Dynamic Symbol Resolution and Symbol Registry for LAR Linker.
//!
//! Handles ELF string tables, symbol tables (linear and GNU Hash), and a global symbol registry.

use crate::linker::elf::{Elf64Sym, STB_GLOBAL, STB_WEAK};
use std::collections::HashMap;
use std::fmt;

/// Represents a resolved symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub address: usize,
    pub size: usize,
    pub binding: u8,
    pub sym_type: u8,
}

impl Symbol {
    pub fn new(name: impl Into<String>, address: usize, size: usize, binding: u8, sym_type: u8) -> Self {
        Self {
            name: name.into(),
            address,
            size,
            binding,
            sym_type,
        }
    }

    #[inline]
    pub fn is_global(&self) -> bool {
        self.binding == STB_GLOBAL
    }

    #[inline]
    pub fn is_weak(&self) -> bool {
        self.binding == STB_WEAK
    }
}

/// Parsed ELF Dynamic Symbol Table.
#[derive(Debug, Clone)]
pub struct DynamicSymbolTable {
    symbols: Vec<Symbol>,
    name_to_index: HashMap<String, usize>,
}

impl DynamicSymbolTable {
    /// Parses symbol table from memory buffers.
    pub fn parse(
        symtab_data: &[u8],
        strtab_data: &[u8],
        load_base: usize,
    ) -> Self {
        let sym_size = 24; // sizeof(Elf64_Sym)
        let count = symtab_data.len() / sym_size;
        let mut symbols = Vec::with_capacity(count);
        let mut name_to_index = HashMap::with_capacity(count);

        for i in 0..count {
            let start = i * sym_size;
            let end = start + sym_size;
            if let Ok(sym) = Elf64Sym::parse(&symtab_data[start..end]) {
                // If past entry 0 and all fields are zero, we've reached uninitialized trailing space
                if i > 0 && sym.st_name == 0 && sym.st_value == 0 && sym.st_size == 0 && sym.st_info == 0 {
                    break;
                }

                let name = Self::extract_string(strtab_data, sym.st_name as usize);
                let address = if sym.st_value != 0 {
                    load_base + sym.st_value as usize
                } else {
                    0
                };

                let symbol = Symbol {
                    name: name.clone(),
                    address,
                    size: sym.st_size as usize,
                    binding: sym.binding(),
                    sym_type: sym.sym_type(),
                };

                if !name.is_empty() && (symbol.is_global() || symbol.is_weak()) {
                    name_to_index.insert(name, symbols.len());
                }

                symbols.push(symbol);
            }
        }

        Self {
            symbols,
            name_to_index,
        }
    }

    /// Looks up a symbol by name.
    pub fn lookup(&self, name: &str) -> Option<&Symbol> {
        self.name_to_index.get(name).map(|&idx| &self.symbols[idx])
    }

    /// Returns a symbol at a specific symbol index.
    pub fn get(&self, index: usize) -> Option<&Symbol> {
        self.symbols.get(index)
    }

    /// Extracts a null-terminated string from string table.
    pub fn extract_string(strtab: &[u8], offset: usize) -> String {
        if offset >= strtab.len() {
            return String::new();
        }
        let end = strtab[offset..]
            .iter()
            .position(|&b| b == 0)
            .map(|pos| offset + pos)
            .unwrap_or(strtab.len());

        String::from_utf8_lossy(&strtab[offset..end]).to_string()
    }
}

/// Global Symbol Registry providing resolution across loaded libraries and host shims.
#[derive(Default)]
pub struct SymbolRegistry {
    symbols: HashMap<String, usize>,
}

impl SymbolRegistry {
    pub fn new() -> Self {
        Self {
            symbols: HashMap::new(),
        }
    }

    /// Registers a symbol with a given address.
    pub fn register(&mut self, name: impl Into<String>, address: usize) {
        self.symbols.insert(name.into(), address);
    }

    /// Resolves a symbol address by name.
    pub fn resolve(&self, name: &str) -> Option<usize> {
        self.symbols.get(name).copied()
    }

    /// Registers all exported global symbols from a DynamicSymbolTable.
    pub fn register_symbol_table(&mut self, symtab: &DynamicSymbolTable) {
        for sym in &symtab.symbols {
            if !sym.name.is_empty() && sym.address != 0 && (sym.is_global() || sym.is_weak()) {
                self.symbols.insert(sym.name.clone(), sym.address);
            }
        }
    }

    /// Total number of registered symbols.
    pub fn count(&self) -> usize {
        self.symbols.len()
    }
}

impl fmt::Debug for SymbolRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SymbolRegistry")
            .field("registered_symbols_count", &self.symbols.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_string() {
        let strtab = b"\0hello\0world\0";
        assert_eq!(DynamicSymbolTable::extract_string(strtab, 1), "hello");
        assert_eq!(DynamicSymbolTable::extract_string(strtab, 7), "world");
        assert_eq!(DynamicSymbolTable::extract_string(strtab, 0), "");
    }

    #[test]
    fn test_symbol_registry() {
        let mut registry = SymbolRegistry::new();
        registry.register("malloc", 0x1000);
        registry.register("free", 0x2000);

        assert_eq!(registry.resolve("malloc"), Some(0x1000));
        assert_eq!(registry.resolve("free"), Some(0x2000));
        assert_eq!(registry.resolve("missing"), None);
    }
}
