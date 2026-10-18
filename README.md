# Buckshot Equestria

> [!IMPORTANT]
> **📜 LICENSE → [`LICENSE`](LICENSE) · Third-party rights → [`NOTICE.md`](NOTICE.md)**
>
> **EN:** Code is MIT and the story is CC BY-NC-SA 4.0. The **MLP files in `assets/mlp/` are NOT licensed**: they belong to Hasbro / DHX and are kept here only as a single place to get everything at once. Read [LICENSE](LICENSE) §3 and [NOTICE.md](NOTICE.md) before using them.
>
> **RU:** Код под MIT, сюжет под CC BY-NC-SA 4.0. **Файлы MLP в `assets/mlp/` НЕ лицензируются**: они принадлежат Hasbro / DHX и лежат здесь только как единое место, откуда можно взять всё сразу. Перед использованием прочитай [LICENSE](LICENSE) §3 и [NOTICE.md](NOTICE.md).

**EN:** A fan game on the AlKAsH3D engine: Buckshot Roulette against ponies, with
29 opponents over 26 rounds. The final opponent is Twilight Sparkle.

**RU:** Фанатская игра на движке AlKAsH3D: Buckshot Roulette против пони,
29 соперников, 26 раундов. Финальная соперница — Искорка.

| Path / Где | What / Что |
|---|---|
| `src/buckshot/` | Buckshot Roulette port (bin `buckshot`) / порт Buckshot Roulette |
| `src/restroom/` | restroom → void with the table, story (bin `restroom`) / туалет → пустота со столом, сюжет |
| `src/ponies/` | 2D pony puppets, expressions, turns (bin `ponies`) / 2D-паппеты пони, эмоции, повороты |
| `scripts/void_lamps.lua` | lamps in the void (Lua, alkash3d-luascript runtime) / лампы в пустоте |
| `story/` | lines: `equestria_lines.txt`, smoker `restroom_lines.txt`; pony layout `ponies.txt` / реплики, курильщик, расстановка пони |
| `tools/mlp/` | FLA → `.mlpo` export, turns (JSFL for Animate), `turn3d/` / экспорт и повороты |
| `assets/mlp/` | MLP files: `.fla` + `.mlpo` pack, Git LFS / файлы MLP, Git LFS |

## Build & run / Сборка и запуск

**EN:** Build from `alkash3d-rust`; the bin paths are in its `Cargo.toml`.
**RU:** Собирать из `alkash3d-rust`, пути бинов прописаны в его `Cargo.toml`.

```
git lfs pull
buckshot-equestria/assets/mlp/pack/join.bat
cd alkash3d-rust
cargo run --release --bin restroom      # buckshot, ponies
```

**EN:** `restroom` takes `mouse.rs` from `alkash3d-rust/src/bin/msc/`. The game reads
its lines from `data/buckshot/void/` and `data/buckshot/restroom/`; the copies in
`story/` are the versions tracked in git.

**RU:** `restroom` берёт `mouse.rs` из `alkash3d-rust/src/bin/msc/`. Реплики игра
читает из `data/buckshot/void/` и `data/buckshot/restroom/`; файлы в `story/` —
версии, которые лежат в git.

## Legal / Права

**EN:** [LICENSE](LICENSE): code under MIT, story under CC BY-NC-SA 4.0. The MLP
files are not licensed by the author. See also [NOTICE.md](NOTICE.md) for
third-party rights and [ASSETS.md](ASSETS.md) for the asset list.

**RU:** [LICENSE](LICENSE): код под MIT, сюжет под CC BY-NC-SA 4.0. Файлы MLP
автор не лицензирует. Права третьих лиц описаны в [NOTICE.md](NOTICE.md), список
ассетов — в [ASSETS.md](ASSETS.md).
