import pygame
import numpy as np
import nes_core
import sys
import time

# Frequência exata do NES NTSC: 1.789773 MHz CPU / 29780.5 ciclos por frame
TARGET_FPS      = 60.098
SAMPLES_PER_FRAME = round(44100 / TARGET_FPS)  # 734 amostras

pygame.init()
pygame.mixer.init(frequency=44100, size=-16, channels=2, buffer=512)

SCALE = 3
WIN_W, WIN_H = 256 * SCALE, 240 * SCALE
screen = pygame.display.set_mode((WIN_W, WIN_H))
pygame.display.set_caption("NES Emulator")
clock = pygame.time.Clock()

DIAG = "--diag" in sys.argv
rom_args = [a for a in sys.argv[1:] if not a.startswith("--")]
if not rom_args:
    print("Uso: python main.py <rom.nes> [--diag]")
    sys.exit(1)

nes = nes_core.Nes()
nes.load_rom(rom_args[0])

# Canal de áudio dedicado — evita proliferação de canais abertos
# Estratégia: play() se livre, queue() se tem 1 som tocando, drop se fila cheia
audio_channel = pygame.mixer.Channel(0)

def play_audio(samples):
    if not samples:
        return
    # Descarta excesso para evitar drift acumulativo.
    # Mais de SAMPLES_PER_FRAME+20 indica que o APU correu além do esperado.
    if len(samples) > SAMPLES_PER_FRAME + 20:
        samples = samples[:SAMPLES_PER_FRAME]
    arr = np.array(samples, dtype=np.float32)
    arr = np.clip(arr, -1.0, 1.0)
    arr_int16 = (arr * 32767).astype(np.int16)
    arr_stereo = np.column_stack([arr_int16, arr_int16])
    sound = pygame.sndarray.make_sound(arr_stereo)
    if not audio_channel.get_busy():
        # Canal livre: toca imediatamente
        audio_channel.play(sound)
    elif audio_channel.get_queue() is None:
        # Canal ocupado mas sem próximo na fila: enfileira
        audio_channel.queue(sound)
    # else: fila cheia → descarta (backpressure relief)

def read_input():
    keys = pygame.key.get_pressed()
    buttons = 0
    if keys[pygame.K_z]:          buttons |= 0x80  # A
    if keys[pygame.K_x]:          buttons |= 0x40  # B
    if keys[pygame.K_RSHIFT]:     buttons |= 0x20  # Select
    if keys[pygame.K_RETURN]:     buttons |= 0x10  # Start
    if keys[pygame.K_UP]:         buttons |= 0x08  # Up
    if keys[pygame.K_DOWN]:       buttons |= 0x04  # Down
    if keys[pygame.K_LEFT]:       buttons |= 0x02  # Left
    if keys[pygame.K_RIGHT]:      buttons |= 0x01  # Right
    return buttons

# --- Diagnóstico ---
frame_count = 0
fps_timer   = time.perf_counter()
acc_core = acc_render = acc_audio = 0.0

running = True
while running:
    for event in pygame.event.get():
        if event.type == pygame.QUIT:
            running = False

    nes.set_input(read_input())

    # --- Core Rust (tempo medido internamente em Rust) ---
    acc_core += nes.step_frame_timed()

    # --- Vídeo ---
    t0 = time.perf_counter()
    buf = bytes(nes.get_framebuffer())
    tmp = pygame.image.frombuffer(buf, (256, 240), "RGB").convert()
    pygame.transform.scale(tmp, (WIN_W, WIN_H), screen)
    pygame.display.flip()
    acc_render += (time.perf_counter() - t0) * 1e6

    # --- Áudio ---
    t0 = time.perf_counter()
    play_audio(nes.get_audio_samples())
    acc_audio += (time.perf_counter() - t0) * 1e6

    frame_count += 1

    # Atualiza título a cada segundo (sempre visível)
    now     = time.perf_counter()
    elapsed = now - fps_timer
    if elapsed >= 1.0:
        fps         = frame_count / elapsed
        queue_depth = 1 if audio_channel.get_queue() is not None else 0
        pygame.display.set_caption(
            f"NES  {fps:.1f} fps  |  "
            f"core {acc_core/frame_count:.0f}µs  "
            f"render {acc_render/frame_count:.0f}µs  "
            f"audio {acc_audio/frame_count:.0f}µs  "
            f"queue {queue_depth}"
        )
        if DIAG:
            print(
                f"[DIAG] fps={fps:.1f}  "
                f"core={acc_core/frame_count:.0f}µs  "
                f"render={acc_render/frame_count:.0f}µs  "
                f"audio={acc_audio/frame_count:.0f}µs  "
                f"queue={queue_depth}  "
                f"samples_per_frame={SAMPLES_PER_FRAME}"
            )
        frame_count = 0
        fps_timer   = now
        acc_core = acc_render = acc_audio = 0.0

    # tick_busy_loop: busy-wait de alta precisão — não "dorme demais" como tick()
    clock.tick_busy_loop(TARGET_FPS)

pygame.quit()
