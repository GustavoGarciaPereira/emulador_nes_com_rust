import pygame
import nes_core
import sys

pygame.init()
SCALE = 3
screen = pygame.display.set_mode((256 * SCALE, 240 * SCALE))
pygame.display.set_caption("NES Emulator")
clock = pygame.time.Clock()

if len(sys.argv) < 2:
    print("Uso: python main.py <rom.nes>")
    sys.exit(1)

nes = nes_core.Nes()
nes.load_rom(sys.argv[1])

running = True
while running:
    for event in pygame.event.get():
        if event.type == pygame.QUIT:
            running = False

    nes.step_frame()

    buf = bytes(nes.get_framebuffer())
    surface = pygame.image.frombuffer(buf, (256, 240), "RGB")
    scaled = pygame.transform.scale(surface, (256 * SCALE, 240 * SCALE))
    screen.blit(scaled, (0, 0))
    pygame.display.flip()
    clock.tick(60)

pygame.quit()
