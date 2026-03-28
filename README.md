# 🎮 NES Emulator

<div align="center">

A Nintendo Entertainment System emulator written in **Rust** (core) + **Python** (frontend), bridged via PyO3.

[![Rust](https://img.shields.io/badge/Rust-2021_edition-orange?logo=rust)](https://www.rust-lang.org/)
[![Python](https://img.shields.io/badge/Python-3.8%2B-blue?logo=python&logoColor=white)](https://www.python.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Build](https://img.shields.io/badge/build-passing-brightgreen)](https://github.com)
[![nestest](https://img.shields.io/badge/nestest-8991%2F8991_%E2%9C%85-brightgreen)](roms/)
[![PyO3](https://img.shields.io/badge/bridge-PyO3%20%2B%20maturin-blueviolet)](https://pyo3.rs/)

</div>

---

## 📸 Screenshots

<div align="center">

| Mega Man II — título (Mapper 1) | Mario Bros — título (Mapper 0) |
|:---:|:---:|
| ![Mega Man II](docs/image.png) | ![Mario Bros título](docs/image%20copy.png) |

| Mario Bros — gameplay |
|:---:|
| ![Mario Bros gameplay](docs/image%20copy%202.png) |

</div>

---

## ✨ Features

| Componente | Status | Detalhes |
|---|---|---|
| **CPU MOS 6502** | ✅ Completa | Todos os 56 opcodes oficiais + ilegais (LAX, SAX, DCP, ISC, RLA, SLO, SRE, RRA). Validada com **nestest 8991/8991** |
| **PPU** | ✅ Completa | Background com scroll completo, sprites, OAM DMA, Sprite 0 Hit, VBlank/NMI |
| **APU** | ✅ Implementada | Pulse 1 & 2, Triangle, Noise — frame counter 4/5-step, mixing linear |
| **Input** | ✅ Implementado | Protocolo serial NES, mapeamento completo de teclado |
| **Mapper 0** | ✅ | NROM (16 KB espelhado e 32 KB) |
| **Mapper 1** | ✅ | MMC1 — PRG/CHR bank switching 4 modos, CHR-RAM, mirroring dinâmico |
| **Frontend** | ✅ | Pygame, escala 3×, ~60 fps, áudio 44100 Hz estéreo |
| **Bridge** | ✅ | PyO3 + maturin — módulo `.so` importável pelo Python |

---

## 🕹️ Jogos Compatíveis

Testados e funcionando:

| Jogo | Mapper |
|---|---|
| Super Mario Bros | Mapper 0 |
| Donkey Kong | Mapper 0 |
| Pac-Man | Mapper 0 |
| Mario Bros | Mapper 0 |
| Mega Man 2 | Mapper 1 (MMC1) |
| The Legend of Zelda | Mapper 1 (MMC1) |
| Metroid | Mapper 1 (MMC1) |

> ROMs proprietárias não estão incluídas no repositório. Use apenas ROMs que você possui legalmente.

---

## 📋 Requisitos

- **Rust** 1.70+ com `cargo`
- **Python** 3.8+
- **maturin** (`pip install maturin`)
- **pygame** (`pip install pygame`)
- **numpy** (`pip install numpy`)

---

## 🚀 Instalação e Uso

```bash
# 1. Clone o repositório
git clone https://github.com/GustavoGarciaPereira/emulador_nes_com_rust.git
cd emulador_nes_com_rust

# 2. Crie e ative o ambiente virtual Python
python -m venv venv
source venv/bin/activate          # Linux/macOS
# venv\Scripts\activate           # Windows

# 3. Instale as dependências Python
pip install maturin pygame numpy

# 4. Compile o core Rust e instale no venv
maturin develop

# 5. Execute com uma ROM
python main.py roms/jogo.nes
```

Para máxima performance, compile em modo release:

```bash
maturin develop --release
python main.py roms/jogo.nes
```

---

## ⌨️ Controles

| Tecla | Botão NES |
|---|---|
| `Z` | A |
| `X` | B |
| `Enter` | Start |
| `R Shift` | Select |
| `↑ ↓ ← →` | D-Pad |
| `Esc` | Sair |

---

## 🏗️ Arquitetura

O emulador usa uma arquitetura híbrida: o core de hardware roda em Rust para máxima performance, e o frontend em Python para facilidade de desenvolvimento.

```
┌─────────────────────────────────────────────────────────────┐
│                        main.py (Python)                      │
│                                                              │
│   Pygame Window   ──►  Keyboard Input  ──►  Audio Output    │
│        │                    │                    ▲           │
│        │ get_framebuffer()  │ set_input()        │           │
│        │                    │        get_audio_samples()     │
└────────┼────────────────────┼────────────────────┼──────────┘
         │                    │                    │
         │        PyO3 + maturin (.so bridge)      │
         │                    │                    │
┌────────▼────────────────────▼────────────────────┼──────────┐
│                      nes_core (Rust)              │          │
│                                                   │          │
│  ┌──────────┐   ┌──────────┐   ┌──────────┐  ┌──┴─────┐   │
│  │  CPU     │   │  PPU     │   │  APU     │  │  Bus   │   │
│  │ MOS 6502 │◄──│ 256×240  │   │ 4 canais │  │ memory │   │
│  │ + ilegais│   │ bg+sprite│   │ 44100 Hz │  │  map   │   │
│  └────┬─────┘   └────┬─────┘   └──────────┘  └──┬─────┘   │
│       │              │                            │          │
│       └──────────────┴────────────────────────────┘          │
│                             │                                 │
│                    ┌────────▼────────┐                       │
│                    │   Cartridge     │                       │
│                    │  Mapper 0 NROM  │                       │
│                    │  Mapper 1 MMC1  │                       │
│                    └─────────────────┘                       │
└─────────────────────────────────────────────────────────────┘
```

### Componentes Rust (`src/`)

| Arquivo | Responsabilidade |
|---|---|
| `lib.rs` | Bridge PyO3 — expõe `struct Nes` ao Python |
| `cpu.rs` | CPU MOS 6502 completa |
| `bus.rs` | Barramento de memória, mapa de endereços |
| `ppu.rs` | PPU — rendering, VBlank, NMI |
| `apu.rs` | APU — síntese de áudio |
| `cartridge.rs` | Parser iNES, Mapper 0, Mapper 1/MMC1 |

---

## 🗺️ Roadmap

- [ ] **Mapper 2** (UxROM) — Mega Man, Castlevania, Contra
- [ ] **Mapper 4** (MMC3) — Super Mario Bros 3, Mega Man 3–6
- [ ] **APU DMC** — canal de sample delta (PCM)
- [ ] **Save States** — salvar e carregar estado completo
- [ ] **Segundo controle** — suporte ao controlador 2
- [ ] **Sprites 8×16** — modo de sprites altos do PPU
- [ ] **Correção de timing** — ciclos exatos por instrução

---

## 📚 Referências

- [NESDev Wiki](https://www.nesdev.org/wiki/) — referência primária para todo o hardware NES
- [6502 opcodes oficiais](https://www.nesdev.org/obelisk-6502-guide/reference.html)
- [6502 opcodes ilegais](https://www.nesdev.org/wiki/CPU_unofficial_opcodes)
- [Formato iNES](https://www.nesdev.org/wiki/INES)
- [PPU rendering](https://www.nesdev.org/wiki/PPU_rendering)
- [nestest ROM](http://www.qmtpro.com/~nes/misc/nestest.txt) — suite de testes da CPU
- [PyO3](https://pyo3.rs/) — bindings Rust/Python
- [maturin](https://www.maturin.rs/) — build tool para extensões Python em Rust

---

## 📄 Licença

Este projeto está licenciado sob a [MIT License](LICENSE).

---

<div align="center">
Feito com ♥ em Rust + Python
</div>
