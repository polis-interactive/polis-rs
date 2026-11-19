MEMORY {
    BOOT2 : ORIGIN = 0x10000000, LENGTH = 0x100
    FLASH : ORIGIN = 0x10000100, LENGTH = 2048K - 16K - 0x100
    RAM : ORIGIN = 0x20000000, LENGTH = 264K
}

/* Example-only partition; firmware cannot grow into these four sectors. */
__settings_start = 0x10000000 + 2048K - 16K;
__settings_end = 0x10000000 + 2048K;
ASSERT(ORIGIN(FLASH) + LENGTH(FLASH) == __settings_start, "settings partition overlaps firmware");
