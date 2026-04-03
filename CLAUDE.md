# CLAUDE.md — Manual de Bordo: Emulador de NES (Rust + Python)

## Visão Geral

Emulador do Nintendo Entertainment System (NES) com arquitetura híbrida:
- **Rust** implementa o core do emulador (CPU, PPU, APU, Bus, Cartridge) com máxima performance.
- **Python** cuida do frontend: janela, input do usuário e rendering via Pygame.
- **Bridge** entre os dois via [PyO3](https://pyo3.rs/) + [maturin](https://www.maturin.rs/), gerando uma biblioteca `.so` importável pelo Python.

O objetivo é ter um emulador funcional capaz de rodar ROMs NES reais, com suporte aos mappers 0 (NROM), 1 (MMC1) e 2 (UxROM).

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
    ├── cartridge.rs    # Parser iNES + Mapper 0 (NROM) + Mapper 1 (MMC1) + Mapper 2 (UxROM)
    ├── ppu.rs          # PPU: rendering de background, VBlank, NMI
    └── apu.rs          # APU: Pulse 1, Pulse 2, Triangle, Noise
```

> **Estado atual:** CPU, Bus, Cartridge (Mapper 0 + Mapper 1/MMC1 + Mapper 2/UxROM), PPU, Input e APU implementados.
> O pipeline completo funciona: `step_frame()` roda um frame inteiro, `get_framebuffer()` retorna o buffer RGB e `get_audio_samples()` retorna amostras f32 para o Pygame.
> Próxima etapa: suporte a mappers adicionais (MMC3/Mapper 4, etc.).

---

## Comandos Essenciais

```bash
# Ativar o ambiente virtual Python
source venv/bin/activate

# Compilar o módulo Rust e instalar no venv (SEMPRE usar --release)
maturin develop --release

# Rodar o emulador com uma ROM
python main.py roms/jogo.nes

# Rodar com diagnóstico detalhado no terminal (FPS, core, render, audio µs)
python main.py roms/jogo.nes --diag

# Validar a CPU contra o log de referência (8991 instruções)
python tools/nestest.py

# Testar carregamento de ROM via load_rom()
python tools/test_cartridge.py
```

> **IMPORTANTE:** Nunca usar `maturin develop` sem `--release`. O modo debug desativa todas as otimizações LLVM — o core foi medido em ~15 ms/frame no modo debug vs ~1–2 ms/frame em release (diferença de 8–15×).

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

| Range                         | Destino                                                      |
|-------------------------------|--------------------------------------------------------------|
| 0x0000–0x1FFF                 | RAM interna 2 KB (espelhada via & 0x07FF)                    |
| 0x2000–0x3FFF                 | PPU registers — delegado a `Ppu::read/write_register`        |
| 0x4014                        | OAM DMA — copia 256 bytes da RAM page para `ppu.oam`        |
| 0x4015 (leitura)              | APU status — stub (retorna 0)                               |
| 0x4016 (leitura)              | Controlador 1 — serial (strobe + shift reg)                 |
| 0x4017 (leitura)              | Controlador 2 — stub (retorna 0x40)                         |
| 0x4000–0x4013, 0x4015, 0x4017 (escrita) | APU — `apu.write(addr, value)`               |
| 0x4016 (escrita)              | Controlador — strobe                                        |
| 0x4018–0x401F                 | Expansão — ignorado                                         |
| 0x4020–0x7FFF                 | Expansão / SRAM (stub — retorna 0)                           |
| 0x8000–0xFFFF                 | PRG-ROM via `Cartridge::read_prg()`; escritas vão para `Cartridge::write_prg()` (mapper) |

- `Bus::read` é `&mut self` (registradores PPU têm side effects na leitura)
- `Bus` possui `pub ppu: Ppu`, `pub apu: Apu`, `controller1`, `controller1_shift`, `controller_strobe`
- OAM DMA lê diretamente de `self.ram` (evita recursão em `self.read`)
- Escritas em 0x8000–0xFFFF chamam `cart.write_prg()` e sincronizam o estado MMC1 (`mmc1_chr0/chr1/control/mirroring`) para a PPU via variáveis locais (evita conflito de borrow entre `self.cartridge` e `self.ppu`)

### ✅ Cartridge + Mapper 0 + Mapper 1/MMC1 + Mapper 2/UxROM (`src/cartridge.rs`) — IMPLEMENTADO

**Parser iNES (header 16 bytes):**
- Valida magic `NES\x1A`
- Lê PRG-ROM (byte 4 × 16 KB) e CHR-ROM (byte 5 × 8 KB)
- Extrai mapper = `(flags7 & 0xF0) | (flags6 >> 4)`
- Lê mirroring (horizontal / vertical / four-screen) — `Mirroring` é `Copy`
- Desconta trainer opcional (512 bytes, bit 2 do flags6)
- Aceita Mapper 0, Mapper 1 e Mapper 2; retorna `Err` para outros
- Aloca `chr_ram: Vec<u8>` de 8 KB quando `chr_size == 0` (CHR-RAM)
- **Validado: nestest.nes carrega, reset vector 0xC004 lido corretamente ✅**

**Mapper 0 (NROM):**
- `read_prg` usa `addr % prg_rom.len()` — cobre NROM-128 (16 KB, espelhado) e NROM-256 (32 KB)

**Mapper 1 (MMC1):**

Estado interno na struct `Cartridge`: `mmc1_shift` (5 bits), `mmc1_shift_count`, `mmc1_control`, `mmc1_chr0`, `mmc1_chr1`, `mmc1_prg`.

`write_prg(addr, value)` — shift register serial:
- Bit 7 setado: reset (`shift=0`, `count=0`, `control |= 0x0C`)
- Acumula 5 bits LSB; ao completar, despacha pelo endereço:
  - `0x8000–0x9FFF`: `mmc1_control` → atualiza `mirroring` (SingleScreenLow/High/Vertical/Horizontal)
  - `0xA000–0xBFFF`: `mmc1_chr0`
  - `0xC000–0xDFFF`: `mmc1_chr1`
  - `0xE000–0xFFFF`: `mmc1_prg` (bits 0–3)

`read_prg` (PRG bank switching via `mmc1_control` bits 2–3):
| Mode | Lo (0x8000–0xBFFF) | Hi (0xC000–0xFFFF) |
|------|--------------------|--------------------|
| 0/1  | `mmc1_prg & 0xFE`  | `mmc1_prg \| 0x01` (32 KB) |
| 2    | fixo no banco 0    | `mmc1_prg`         |
| 3    | `mmc1_prg`         | fixo no último banco |

`read_chr` / `write_chr` (CHR bank switching via `mmc1_control` bit 4):
- CHR-RAM: usado quando `chr_rom.is_empty()`
- Modo 8 KB (`chr_mode=0`): `bank = mmc1_chr0 & 0xFE`, mapeado em 0x0000–0x1FFF
- Modo 4 KB (`chr_mode=1`): `mmc1_chr0` → 0x0000–0x0FFF, `mmc1_chr1` → 0x1000–0x1FFF

**`Mirroring` enum** — 5 variantes:
```rust
Horizontal, Vertical, FourScreen, SingleScreenLow, SingleScreenHigh
```

**Mapper 2 (UxROM):**

Estado interno na struct `Cartridge`: `uxrom_prg_bank: u8` (banco selecionável, inicializado em 0).

`write_prg(addr, value)` — qualquer escrita em 0x8000–0xFFFF armazena `value` em `uxrom_prg_bank` diretamente (o `% prg_banks` em `read_prg` trata overflow). Curto-circuita antes da lógica MMC1.

`read_prg`:
| Janela | Banco |
|--------|-------|
| 0x8000–0xBFFF | `uxrom_prg_bank % prg_banks` (selecionável) |
| 0xC000–0xFFFF | último banco — sempre fixo |

`read_chr`: CHR fixo em 8 KB — compartilha o mesmo arm do Mapper 0 (`0 | 2 =>`). Usa `chr_rom[addr & 0x1FFF]` se disponível, senão `chr_ram` (CHR-RAM de 8 KB alocada no load).

Jogos notáveis que usam UxROM: Mega Man, Contra, Castlevania, DuckTales.

### ✅ PPU (`src/ppu.rs`) — COMPLETA (background + sprites + scroll)

**Struct `Ppu`** — campos principais:
- `vram: [u8; 2048]` — nametables
- `palette: [u8; 32]` — paleta interna
- `oam: [u8; 256]` — 64 sprites × 4 bytes (Y, tile, attrs, X)
- `chr_rom: Vec<u8>` — cópia do CHR-ROM do cartucho (evita conflitos de borrow)
- `chr_ram: Vec<u8>` — 8 KB de CHR-RAM (usado quando `chr_rom` está vazio)
- `mirroring: Mirroring` — copiado/sincronizado do cartucho
- `mapper: u8`, `mmc1_chr0: u8`, `mmc1_chr1: u8`, `mmc1_control: u8` — estado MMC1 sincronizado do `Cartridge` pelo `Bus` a cada escrita em 0x8000–0xFFFF
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

**Renderização de background** (`render_background`) — pixel-a-pixel com scroll completo e cache de tile:
- Loop em `0..256 pixels` por scanline
- `x = pixel_x + scroll_x`, `y = scanline + scroll_y`
- Nametable selecionada por `nt_x = (coarse_x/32 ^ ctrl_nt_x) & 1` e `nt_y` análogo — suporta scroll contínuo entre as 4 nametables com wrap correto
- Atributo de paleta: `base_nt + 0x3C0 + attr_y*8 + attr_x`
- **Otimização de cache por tile:** termos que dependem só de `y` (coarse_y, fine_y, nt_y, attr_y, nt_row) são calculados 1×/scanline. Dados do tile (tile_idx, palette_idx, pattern_lo/hi, tile_colors[4]) são recalculados apenas quando `coarse_x` muda (a cada 8 pixels). No loop interno de pixel, zero chamadas a `ppu_read`. Resultado: ~245.760 → ~31.680 chamadas `ppu_read`/frame (**≈ 8× menos**).

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

**Memória PPU** (`ppu_read` / `write_register` PPUDATA):

| Range         | Fonte                                                          |
|---------------|----------------------------------------------------------------|
| 0x0000–0x1FFF | CHR-ROM com bank switching MMC1, ou CHR-RAM se rom vazia       |
| 0x2000–0x3EFF | VRAM com mirroring H/V/FourScreen/SingleScreenLow/High         |
| 0x3F00–0x3FFF | Palette RAM (32 bytes, espelhada)                              |

- Escritas via PPUDATA (0x2007) para 0x0000–0x1FFF gravam em `chr_ram` quando disponível

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

### ✅ APU (`src/apu.rs`) — IMPLEMENTADO

**Canais:**

| Canal    | Struct    | Detalhes                                                                 |
|----------|-----------|--------------------------------------------------------------------------|
| Pulse 1  | `Pulse`   | Duty cycle (4 padrões), envelope, length counter, sweep (negate com −1 extra) |
| Pulse 2  | `Pulse`   | Igual ao Pulse 1; sweep negate sem o −1 extra                           |
| Triangle | `Triangle`| Sequência fixa 32 passos, linear counter + length counter; timer_period < 2 silenciado |
| Noise    | `Noise`   | LFSR 15-bit, modo normal (bit 1) e curto (bit 6), envelope + length counter |

**Frame counter** (`frame_mode` = false → 4-step, true → 5-step):

| Ciclo CPU | 4-step          | 5-step          |
|-----------|-----------------|-----------------|
| 3729      | quarter         | quarter         |
| 7457      | quarter + half  | quarter + half  |
| 11186     | quarter         | quarter         |
| 14915     | quarter + half + reset | —        |
| 18641     | —               | quarter + half + reset |

- **Quarter frame:** clock envelope (Pulse 1, Pulse 2, Noise) + linear counter (Triangle)
- **Half frame:** clock length counters + sweep (Pulse 1, Pulse 2, Triangle, Noise)
- Sweep mute: `timer_period < 8` ou overflow (`target > 0x7FF` no modo positivo)
- Timer do Triangle cloca a cada ciclo de CPU; Pulse e Noise clocam a cada 2 ciclos (divisor APU)

**Geração de áudio:**
- Taxa: 44100 Hz (acumulador `sample_accum += 44100.0 / 1_789_773.0` por ciclo de CPU)
- ~733 amostras f32 por frame
- Mixing linear: `pulse_out = 0.00752 × (p1 + p2)`, `tnd_out = 0.00851 × tri + 0.00494 × noise`

**Mapeamento de registradores (Bus):**

| Range                    | Destino             |
|--------------------------|---------------------|
| 0x4000–0x4013, 0x4015, 0x4017 | `apu.write(addr, value)` |
| Leitura 0x4015           | 0 (stub)            |

**Frontend (main.py):**
- `pygame.mixer.init(frequency=44100, size=-16, channels=2, buffer=512)`
- Canal de áudio dedicado (`pygame.mixer.Channel(0)`) com estratégia de backpressure:
  - Canal livre → `channel.play(sound)` (toca imediatamente)
  - Canal ocupado, fila vazia → `channel.queue(sound)` (enfileira próximo)
  - Canal ocupado, fila cheia → descarta o frame de áudio (previne acúmulo)
- Cap de amostras: `SAMPLES_PER_FRAME = round(44100 / 60.098) = 734` — excesso descartado antes de criar o Sound
- `play_audio`: converte f32 → int16 → stereo com `np.column_stack` → `pygame.sndarray.make_sound`
- Dependência: `numpy` (instalada no venv)

---

## API PyO3 Atual (`src/lib.rs`)

```python
from nes_core import Nes

nes = Nes()                    # construtor vazio (para testes unitários)
nes.load_rom("roms/rom.nes")   # carrega ROM (mapper 0, 1 ou 2), copia CHR-ROM/RAM + estado MMC1 para PPU, faz reset

nes.reset()                    # re-executa o reset vector
nes.step()                     # executa 1 instrução + 3 ciclos PPU/ciclo + verifica NMI
nes.step_frame()               # executa instruções até completar um frame inteiro
nes.step_frame_timed()         # igual a step_frame(), mas retorna tempo de execução em µs (u64)

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
nes.get_audio_samples()           # -> Vec<f32>  (~734 amostras por frame a 44100 Hz; esvazia o buffer)
```

> **Próximo passo:** suporte a mappers adicionais (MMC3/Mapper 4, UxROM-variant/Mapper 94, etc.).

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

### PPU — Decisão de Design: CHR na PPU + Sincronização MMC1

A PPU armazena cópias de `chr_rom`, `chr_ram` e `mirroring` do cartucho internamente. Isso evita conflito de borrow no Rust: `Bus::ppu.tick()` precisa de acesso mutável à PPU ao mesmo tempo que poderia precisar ler `Bus::cartridge`.

Para o MMC1, o estado de bank switching CHR (`mmc1_chr0`, `mmc1_chr1`, `mmc1_control`) é armazenado tanto no `Cartridge` quanto na `Ppu`. O `Bus::write` usa variáveis locais para extrair o estado atualizado do cartucho e copiá-lo para a PPU após cada escrita em 0x8000–0xFFFF, mantendo os dois em sincronia sem conflito de borrow.

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

## Performance & Diagnóstico

### Números de referência (maturin develop --release, hardware moderno)

| Métrica | Valor esperado |
|---|---|
| `core` (step_frame_timed) | 1.000–3.000 µs |
| `render` (frombuffer→scale→flip) | 1.500–4.000 µs |
| `audio` (make_sound + queue) | < 500 µs |
| FPS real | 59–61 |

### Sincronização de tempo

O loop principal usa `clock.tick_busy_loop(60.098)` — busy-wait de alta precisão que não "dorme demais" como `tick()` (`SDL_Delay` tem granularidade de ±4 ms). O alvo é 60.098 fps, que corresponde à frequência exata do NES NTSC.

### Pipeline de render Pygame

```python
# .convert() converte a Surface imediatamente para o pixel-format do display
# (sem isso, scale/blit fazem conversão implícita a cada frame)
tmp = pygame.image.frombuffer(buf, (256, 240), "RGB").convert()
# Terceiro argumento = surface de destino → sem alocar Surface intermediária
pygame.transform.scale(tmp, (WIN_W, WIN_H), screen)
```

### Diagnóstico em tempo real

- **Título da janela:** exibe FPS, core µs, render µs, audio µs e profundidade da fila de áudio a cada segundo — sempre ativo.
- **Flag `--diag`:** `python main.py rom.nes --diag` imprime o mesmo no terminal a cada segundo.
- **`step_frame_timed()`** em `src/lib.rs`: método PyO3 que mede o tempo do loop principal em Rust com `std::time::Instant`, retorna `u64` em microssegundos. Útil para isolar se o gargalo é no core Rust ou no frontend Python.

### Backpressure de áudio

Com `tick_busy_loop` na frequência correta o canal de áudio quase nunca entra em drop. Se `queue=1` aparecer consistentemente no título, indica que o emulador está ligeiramente acima de 60 fps — verificar se `TARGET_FPS = 60.098` está sendo respeitado.

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
