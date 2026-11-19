MEMORY {
    FLASH : ORIGIN = 0x10000000, LENGTH = 4096K - 16K
    RAM : ORIGIN = 0x20000000, LENGTH = 512K
}

/* Retain the RP2350 ROM image definition and picotool metadata. */
SECTIONS {
    .start_block : ALIGN(4) {
        __start_block_addr = .;
        KEEP(*(.start_block));
        KEEP(*(.boot_info));
    } > FLASH
} INSERT AFTER .vector_table;
_stext = ADDR(.start_block) + SIZEOF(.start_block);
SECTIONS {
    .end_block : ALIGN(4) {
        __end_block_addr = .;
        KEEP(*(.end_block));
    } > FLASH
} INSERT AFTER .uninit;
PROVIDE(start_to_end = __end_block_addr - __start_block_addr);
PROVIDE(end_to_start = __start_block_addr - __end_block_addr);
ASSERT(__start_block_addr - ORIGIN(FLASH) < 4096, "RP2350 image definition must be in first 4 KiB");

/* Example-only partition; firmware cannot grow into these four sectors. */
__settings_start = 0x10000000 + 4096K - 16K;
__settings_end = 0x10000000 + 4096K;
ASSERT(ORIGIN(FLASH) + LENGTH(FLASH) == __settings_start, "settings partition overlaps firmware");
