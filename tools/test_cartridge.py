import nes_core

nes = nes_core.Nes()
nes.load_rom("roms/nestest.nes")

pc = nes.get_pc()
print(f"PC após reset: {pc:04X}")

# O reset vector real da nestest.nes aponta para 0xC004 (modo automático).
# 0xC000 é o ponto de entrada "manual" usado pelo nestest.py (set_pc).
assert pc == 0xC004, f"Reset vector errado! PC={pc:04X}"
print("✅ Cartridge carregado e reset vector correto! (0xC004 = modo automático nestest)")
