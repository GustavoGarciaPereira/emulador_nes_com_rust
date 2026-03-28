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

running = True
while running:
    for event in pygame.event.get():
        if event.type == pygame.QUIT:
            running = False

    nes.set_input(read_input())
    nes.step_frame()

    buf = bytes(nes.get_framebuffer())
    surface = pygame.image.frombuffer(buf, (256, 240), "RGB")
    scaled = pygame.transform.scale(surface, (256 * SCALE, 240 * SCALE))
    screen.blit(scaled, (0, 0))
    pygame.display.flip()
    clock.tick(60)

pygame.quit()
