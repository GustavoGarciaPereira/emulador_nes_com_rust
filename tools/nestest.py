import nes_core

nes = nes_core.Nes()

# Carrega a ROM via load_rom (usa o sistema de cartridge correto)
nes.load_rom("roms/nestest.nes")

# nestest começa em 0xC000 (modo validação manual)
# O reset vector real é 0xC004 (modo automático), mas o log começa em 0xC000
nes.set_pc(0xC000)

# Ler o log de referência
with open("roms/nestest.log") as f:
    lines = f.readlines()

errors = 0
for i, line in enumerate(lines):
    expected_pc = int(line[0:4], 16)
    current_pc = nes.get_pc()

    if current_pc != expected_pc:
        print(f"ERRO na linha {i+1}:")
        print(f"  Esperado PC: {expected_pc:04X}")
        print(f"  Obtido   PC: {current_pc:04X}")
        print(f"  Log: {line.strip()}")
        errors += 1
        if errors >= 5:
            print("Muitos erros, abortando.")
            break

    nes.step()

if errors == 0:
    print(f"✅ Passou {len(lines)} instruções sem erros!")
