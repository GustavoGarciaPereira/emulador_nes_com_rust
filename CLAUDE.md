# CLAUDE.md — Manual de Bordo: Emulador de NES (Rust + Python)

## Visão Geral

Emulador do Nintendo Entertainment System (NES) com arquitetura híbrida:
- **Rust** implementa o core do emulador (CPU, PPU, APU, Bus, Cartridge) com máxima performance.
- **Python** cuida do frontend: janela, input do usuário e rendering via Pygame.
- **Bridge** entre os dois via [PyO3](https://pyo3.rs/) + [maturin](https://www.maturin.rs/), gerando uma biblioteca `.so` importável pelo Python.

O objetivo é ter um emulador funcional capaz de rodar ROMs NES reais, começando pelo suporte ao mapper 0 (NROM).

---

## Stack Tecnológica

| Camada      | Tecnologia                          |
|-------------|-------------------------------------|
| Core        | Rust (edition 2021), crate-type `cdylib` |
| Bridge      | PyO3 (feature `extension-module`), maturin |
| Frontend    | Python 3.12, Pygame                 |
| Build       | maturin (gerencia compilação e instalação do módulo) |

---

## Estrutura de Pastas

```
nes-emulator/
├── CLAUDE.md           # Este arquivo
├── Cargo.toml          # Dependências e config do crate Rust
├── Cargo.lock
├── pyproject.toml      # Config do maturin / build Python
├── main.py             # Ponto de entrada Python (frontend Pygame)
├── venv/               # Ambiente virtual Python
├── roms/               # ROMs de teste (não commitar ROMs proprietárias)
│   ├── nestest.nes     # ROM de teste da CPU
│   └── nestest.log     # Log de referência (8991 instruções)
├── tools/              # Scripts Python de teste e validação
│   ├── nestest.py      # Valida CPU contra nestest.log (usa load_rom + set_pc)
│   └── test_cartridge.py # Testa load_rom() e reset vector
└── src/                # Código Rust
    ├── lib.rs          # Bridge PyO3 — expõe struct Nes ao Python
    ├── cpu.rs          # CPU MOS 6502 (completa: oficiais + ilegais)
    ├── bus.rs          # Barramento de memória com mapa do NES
    ├── cartridge.rs    # Parser iNES + Mapper 0 (NROM)
    └── ppu.rs          # PPU: rendering de background, VBlank, NMI
```

> **Estado atual:** CPU, Bus, Cartridge, Mapper 0 e PPU básica implementados.
> O pipeline completo funciona: `step_frame()` roda um frame inteiro, `get_framebuffer()` retorna o buffer RGB para o Pygame.
> `apu.rs` e input ainda não existem — próximas etapas.

---

## Comandos Essenciais

```bash
# Ativar o ambiente virtual Python
source venv/bin/activate

# Compilar o módulo Rust e instalar no venv (fazer após qualquer mudança em Rust)
maturin develop

# Rodar o emulador com uma ROM
python main.py roms/jogo.nes

# Compilar em modo release (para medir performance real)
maturin develop --release

# Validar a CPU contra o log de referência (8991 instruções)
python tools/nestest.py

# Testar carregamento de ROM via load_rom()
python tools/test_cartridge.py
```

---

## Status de Implementação

### ✅ CPU MOS 6502 (`src/cpu.rs`) — COMPLETA

- Todos os 13 modos de endereçamento implementados
- Todos os opcodes oficiais (56 instruções)
- Todos os opcodes ilegais necessários para o nestest:
  - NOPs extras (implied, immediate, zero page, zero page X, absolute, absolute X)
  - LAX, SAX, SBC ilegal (0xEB)
  - DCP, ISC, RLA, SLO, SRE, RRA
- **Validado: 8991/8991 instruções do nestest.log ✅**
- Bug do 6502 no modo Indirect implementado (page wrap em 0x??FF)
- ADC/SBC com overflow correto
- Método `nmi()` adicionado: salva PC+status na pilha, salta para vetor 0xFFFA/0xFFFB

### ✅ Bus (`src/bus.rs`) — ATUALIZADO

Mapeamento de memória real do NES:

| Range          | Destino                                                      |
|----------------|--------------------------------------------------------------|
| 0x0000–0x1FFF  | RAM interna 2 KB (espelhada via & 0x07FF)                    |
| 0x2000–0x3FFF  | PPU registers — delegado a `Ppu::read/write_register`        |
| 0x4014         | OAM DMA — copia 256 bytes da RAM page para `ppu.oam`        |
| 0x4016         | Controlador 1 — leitura/escrita serial (strobe + shift reg) |
| 0x4017         | Controlador 2 — stub (retorna 0x40)                         |
| 0x4000–0x401F  | APU / IO (stub — retorna 0)                                  |
| 0x4020–0x7FFF  | Expansão / SRAM (stub — retorna 0)                           |
| 0x8000–0xFFFF  | PRG-ROM via `Cartridge::read_prg()`                          |

- `Bus::read` é `&mut self` (registradores PPU têm side effects na leitura)
- `Bus` possui `pub ppu: Ppu`, `controller1`, `controller1_shift`, `controller_strobe`
- OAM DMA lê diretamente de `self.ram` (evita recursão em `self.read`)

### ✅ Cartridge + Mapper 0 (`src/cartridge.rs`) — IMPLEMENTADO

- Parser do formato iNES (header 16 bytes):
  - Valida magic `NES\x1A`
  - Lê PRG-ROM (byte 4 × 16 KB) e CHR-ROM (byte 5 × 8 KB)
  - Extrai mapper = `(flags7 & 0xF0) | (flags6 >> 4)`
  - Lê mirroring (horizontal / vertical / four-screen) — `Mirroring` é `Copy`
  - Desconta trainer opcional (512 bytes, bit 2 do flags6)
- Retorna `Err` para mapper ≠ 0
- `read_prg` usa `addr % prg_rom.len()` — cobre NROM-128 (16 KB, espelhado) e NROM-256 (32 KB)
- **Validado: nestest.nes carrega, reset vector 0xC004 lido corretamente ✅**

### ✅ PPU (`src/ppu.rs`) — COMPLETA (background + sprites + scroll)

**Struct `Ppu`** — campos principais:
- `vram: [u8; 2048]` — nametables
- `palette: [u8; 32]` — paleta interna
- `oam: [u8; 256]` — 64 sprites × 4 bytes (Y, tile, attrs, X)
- `chr_rom: Vec<u8>` — cópia do CHR-ROM do cartucho (evita conflitos de borrow)
- `mirroring: Mirroring` — copiado do cartucho no `load_rom`
- `framebuffer: Vec<u8>` — 256 × 240 × 3 bytes RGB

**Timing** (262 scanlines × 341 ciclos):

| Scanline  | Função                                                                      |
|-----------|-----------------------------------------------------------------------------|
| 0–239     | Visível — `render_background()` + `render_sprites()` ao ciclo 257           |
| 240       | Pós-render (idle)                                                           |
| 241       | Início do VBlank: seta bit 7 de PPUSTATUS, dispara NMI se PPUCTRL bit 7 = 1 |
| 242–260   | VBlank                                                                      |
| 261       | Pré-render: limpa VBlank, Sprite 0 Hit, Sprite Overflow                     |

**Registradores implementados:**

| Addr   | Reg        | Leitura              | Escrita                          |
|--------|------------|----------------------|----------------------------------|
| 0x2000 | PPUCTRL    | —                    | ctrl = value                     |
| 0x2001 | PPUMASK    | —                    | mask = value (bit3=BG, bit4=SPR) |
| 0x2002 | PPUSTATUS  | retorna + limpa VBlank e latches | —                   |
| 0x2003 | OAMADDR    | —                    | oam_addr = value                 |
| 0x2004 | OAMDATA    | oam[oam_addr]        | oam[oam_addr++] = value          |
| 0x2005 | PPUSCROLL  | —                    | scroll_x / scroll_y (toggle)     |
| 0x2006 | PPUADDR    | —                    | vram_addr hi/lo (toggle)         |
| 0x2007 | PPUDATA    | buffered + inc addr  | escreve VRAM/paleta + inc addr   |

**Renderização de background** (`render_background`) — pixel-a-pixel com scroll completo:
- Loop em `0..256 pixels` por scanline (antes era tile-a-tile)
- `x = pixel_x + scroll_x`, `y = scanline + scroll_y`
- Nametable selecionada por `nt_x = (coarse_x/32 ^ ctrl_nt_x) & 1` e `nt_y` análogo — suporta scroll contínuo entre as 4 nametables com wrap correto
- Atributo de paleta: `base_nt + 0x3C0 + attr_y*8 + attr_x`

**Renderização de sprites** (`render_sprites`) — novo:
- Itera OAM de 63→0 (reverso = sprite 0 sobrescreve em empate, prioridade correta)
- Cada sprite: Y, tile, atributos (flip H/V, paleta, prioridade vs BG), X
- Pattern table de sprite: bit 3 de PPUCTRL (`$0000` ou `$1000`)
- Paleta de sprite: `0x3F10 + paleta×4 + cor`
- Flip vertical: `row = 7 - row`; flip horizontal: inverte bit de leitura
- Pixels transparentes (color\_idx == 0) ignorados
- Sprites com `behind_bg = true` (bit 5 de attrs) não são desenhados sobre o BG

**Sprite 0 Hit:**
- Bit 6 do PPUSTATUS setado quando sprite 0 tem pixel opaco na scanline atual
- Limpo na scanline 261 (pré-render), junto com VBlank e Sprite Overflow

**Memória PPU** (`ppu_read`):

| Range         | Fonte                                |
|---------------|--------------------------------------|
| 0x0000–0x1FFF | CHR-ROM (pattern tables)             |
| 0x2000–0x3EFF | VRAM com mirroring H/V/FourScreen    |
| 0x3F00–0x3FFF | Palette RAM (32 bytes, espelhada)    |

**Paleta:** tabela fixa `NES_PALETTE: [(u8,u8,u8); 64]` com as 64 cores NTSC do NES.

### ✅ Input (`src/bus.rs` + `src/lib.rs` + `main.py`) — IMPLEMENTADO

- **Protocolo serial NES:** strobe em 0x4016 trava o estado em `controller1_shift`; cada leitura de 0x4016 devolve 1 bit na ordem A→B→Select→Start→Up→Down→Left→Right
- **`set_input(buttons: u8)`** exposto via PyO3 — Python passa bitmask a cada frame
- **Mapeamento de teclas** (`main.py`): Z=A, X=B, RShift=Select, Enter=Start, setas=direcionais

| Bit | Botão  | Tecla  |
|-----|--------|--------|
| 7   | A      | Z      |
| 6   | B      | X      |
| 5   | Select | RShift |
| 4   | Start  | Enter  |
| 3   | Up     | ↑      |
| 2   | Down   | ↓      |
| 1   | Left   | ←      |
| 0   | Right  | →      |

### ⬜ APU (`src/apu.rs`) — FUTURA

---

## API PyO3 Atual (`src/lib.rs`)

```python
from nes_core import Nes

nes = Nes()                    # construtor vazio (para testes unitários)
nes.load_rom("roms/rom.nes")   # carrega ROM, copia CHR-ROM/mirroring para PPU, faz reset

nes.reset()                    # re-executa o reset vector
nes.step()                     # executa 1 instrução + 3 ciclos PPU/ciclo + verifica NMI
nes.step_frame()               # executa instruções até completar um frame inteiro

nes.get_framebuffer()  # -> Vec<u8>  (256 * 240 * 3 bytes RGB)

nes.get_pc()      # -> u16
nes.get_a()       # -> u8
nes.get_x()       # -> u8
nes.get_y()       # -> u8
nes.get_sp()      # -> u8
nes.get_status()  # -> u8
nes.get_cycles()  # -> u64

nes.mem_read(addr: u16)           # -> u8  (lê da memória mapeada, &mut — tem side effects)
nes.mem_write(addr: u16, val: u8) # escreve na memória mapeada
nes.set_pc(addr: u16)             # força PC (usado pelo nestest.py)
nes.set_input(buttons: u8)        # atualiza estado do controle 1 (bitmask, ver tabela de botões)
```

> **Próximo passo:** APU para áudio.

---

## Detalhes de Implementação Importantes

### CPU — Flags

| Bit | Flag | Descrição                |
|-----|------|--------------------------|
| 7   | N    | Negative                 |
| 6   | V    | Overflow                 |
| 5   | U    | Unused (sempre 1)        |
| 4   | B    | Break                    |
| 3   | D    | Decimal (ignorado no NES)|
| 2   | I    | Interrupt Disable        |
| 1   | Z    | Zero                     |
| 0   | C    | Carry                    |

Estado inicial após reset: `status = 0x24` (U e I setados).

### CPU — Stack

- Localizada em `0x0100–0x01FF`
- SP começa em `0xFD` após reset
- Push: escreve em `0x0100 | sp`, decrementa sp
- Pop: incrementa sp, lê de `0x0100 | sp`

### CPU — Vetores de interrupção

| Vetor   | Endereço        |
|---------|-----------------|
| NMI     | 0xFFFA / 0xFFFB |
| RESET   | 0xFFFC / 0xFFFD |
| IRQ/BRK | 0xFFFE / 0xFFFF |

### PPU — Decisão de Design: CHR-ROM na PPU

A PPU armazena uma cópia do `chr_rom` e `mirroring` do cartucho internamente. Isso evita conflito de borrow no Rust: `Bus::ppu.tick()` precisa de acesso mutável à PPU ao mesmo tempo que poderia precisar ler `Bus::cartridge`. Copiando no `load_rom`, as referências ficam independentes.

### Cartridge — iNES Header

```
Byte 0-3:  "NES\x1A"  (magic)
Byte 4:    PRG-ROM banks × 16 KB
Byte 5:    CHR-ROM banks × 8 KB
Byte 6:    Flags (bit0=mirror, bit2=trainer, bit3=four-screen, bit4-7=mapper low)
Byte 7:    Flags (bit4-7=mapper high)
Bytes 8-15: Padding / extended flags
[Trainer:  512 bytes se bit2 do byte6 estiver setado]
[PRG-ROM:  byte4 × 16384 bytes]
[CHR-ROM:  byte5 × 8192 bytes]
```

### nestest.nes — Observação Importante

- **Reset vector real:** `0xC004` (modo automático — roda testes e escreve resultado em `$02/$03`)
- **Entrada manual para validação:** `0xC000` (usada pelo `tools/nestest.py` via `set_pc`)
- O `nestest.log` começa em `0xC000` — por isso o `nestest.py` força `PC=0xC000`
- `nestest.py` usa `load_rom()` para carregar a ROM (não escreve bytes manualmente)

---

## Convenções de Código

- **Nomenclatura:** `snake_case` em todo o Rust (structs em `PascalCase`, constantes em `SCREAMING_SNAKE_CASE`).
- **Tratamento de erros:** nunca usar `.unwrap()`. Usar `Result<T, E>` e propagar com `?`. Na API PyO3, converter em `PyResult`.
- **Módulos:** cada componente de hardware em seu próprio arquivo `.rs`; `lib.rs` apenas monta e expõe.
- **Aritmética:** sempre usar `.wrapping_add()` / `.wrapping_sub()` — nunca depender de overflow implícito.
- **Mutabilidade no Bus:** `Bus::read` é `&mut self` por causa dos side effects dos registradores PPU. Todos os helpers da CPU que leem do bus (`fetch`, `pop`, `get_address`, etc.) recebem `&mut Bus`.

---

## Timings do Hardware NES

| Parâmetro             | Valor                              |
|-----------------------|------------------------------------|
| CPU                   | MOS 6502 (sem BCD) @ 1.789773 MHz  |
| PPU                   | Roda a 3× a frequência da CPU      |
| Ciclos por frame      | ~29780 ciclos de CPU               |
| FPS                   | ~60.0988 fps (NTSC)                |
| RAM interna           | 2 KB (espelhada em $0000–$1FFF)    |
| VRAM                  | 2 KB (nametables)                  |
| Resolução             | 256 × 240 pixels                   |
| Paleta                | 64 cores (54 distintas visualmente) |

---

## Referências

- **NESDev Wiki:** https://www.nesdev.org/wiki/  — referência primária para todo hardware
- **6502 opcodes oficiais:** https://www.nesdev.org/obelisk-6502-guide/reference.html
- **6502 opcodes ilegais:** https://www.nesdev.org/wiki/CPU_unofficial_opcodes
- **iNES format:** https://www.nesdev.org/wiki/INES
- **PPU rendering:** https://www.nesdev.org/wiki/PPU_rendering
- **nestest ROM + log:** em `roms/` — indispensável para validar a CPU
