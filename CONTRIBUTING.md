# Contribuindo

Obrigado pelo interesse! Este projeto é aberto a contribuições.

## Quem pode fazer o quê

- **Todo mundo pode**: abrir issues, comentar, discutir, enviar pull requests e sugerir melhorias.
- **Apenas o mantenedor ([LuizGustavoWT](https://github.com/LuizGustavoWT)) pode**: fazer *merge* de pull requests e fechar/abrir issues.

Isso significa que você pode contribuir livremente, mas quem decide o que entra no
projeto é o mantenedor. Se você quiser permissão para revisar/aprovar mudanças,
é só pedir.

## Como contribuir

1. **Fork** este repositório.
2. Crie uma branch: `git checkout -b minha-melhoria`.
3. Faça suas mudanças.
4. Rode os testes: `cargo test --workspace`.
5. Rode o linter: `cargo clippy --workspace --all-targets -- -D warnings`.
6. Abra um **pull request** descrevendo o que mudou e por quê.

## Reportando bugs

Abra uma issue com:

- Seu sistema (Linux/macOS/Windows) e versão.
- Versão do plugin.
- O que você esperava vs. o que aconteceu.
- Passos para reproduzir.
- Logs, se houver: `journalctl --user -u soundbar -n 50` (Linux).

## Áudio é um assunto delicado

Se você mexer em `crates/soundbar-audio` ou `crates/soundbar-core`, por favor
inclua testes. A mixagem é onde bugs aparecem como áudio cortado, clippado ou
atrasado na live — e isso é difícil de perceber sem teste automatizado.

Exemplo de teste de mixagem:

```rust
#[test]
fn ganho_do_slot_escala_amostra() {
    let som = dc("a", 64, 10_000);
    let mut m = Mixer::new(1.0, 16);
    m.play(som.clone(), 0.5, true, None).unwrap();
    let mut out = vec![0i16; 64 * 2];
    m.mix_into(&mut out, &lib(vec![som]));
    assert_eq!(out[0], 5_000);
}
```

## Commits

Mensagens no estilo [Conventional Commits](https://www.conventionalcommits.org/):

```
feat: adiciona fade-out no modo toggle
fix: espera o stream ficar Ready antes de escrever
docs: explica a configuracao no OBS
```

## Código de conduta

Seja respeitoso. Discutimos o código, não as pessoas.
