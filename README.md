# asm-check

Un linter sémantique pour l'assembleur x86-64 en syntaxe Intel (NASM), écrit en Rust.
L'idée : l'équivalent de `cargo check` / `clippy` pour l'asm, avec des diagnostics
affichés directement dans l'IDE.

## Lints prévus

- `div` / `idiv` sans `rdx` initialisé
- deux écritures dans le même registre sans lecture entre les deux
- registre lu après un `call` ou `syscall` qui l'écrase
- registre callee-saved modifié sans sauvegarde
- `push` / `pop` déséquilibrés avant `ret`
- flags écrasés entre un `cmp` et son saut conditionnel
- fonction sans `ret`
- petites optimisations (`mov rax, 0` → `xor eax, eax`, `cmp reg, 0` → `test reg, reg`)

## Utilisation

```bash
cargo run -- fichier.asm
```
