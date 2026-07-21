local ffi = require("ffi")

ffi.cdef[[
uintptr_t op_memory_module_base(void);
uint8_t op_memory_read_u8(uintptr_t address);
uint16_t op_memory_read_u16(uintptr_t address);
uint32_t op_memory_read_u32(uintptr_t address);
uintptr_t op_memory_read_usize(uintptr_t address);
int8_t op_memory_read_i8(uintptr_t address);
int16_t op_memory_read_i16(uintptr_t address);
int32_t op_memory_read_i32(uintptr_t address);
intptr_t op_memory_read_isize(uintptr_t address);
void op_memory_write_u8(uintptr_t address, uint8_t value);
void op_memory_write_u16(uintptr_t address, uint16_t value);
void op_memory_write_u32(uintptr_t address, uint32_t value);
void op_memory_write_usize(uintptr_t address, uintptr_t value);
void op_memory_write_i8(uintptr_t address, int8_t value);
void op_memory_write_i16(uintptr_t address, int16_t value);
void op_memory_write_i32(uintptr_t address, int32_t value);
void op_memory_write_isize(uintptr_t address, intptr_t value);
]]