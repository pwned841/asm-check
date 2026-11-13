; Prints argc in decimal. Uses a helper with its own calling convention.
global _start

section .text
_start:
    mov rax, [rsp]
    mov r12, rax
    lea rdi, [rel buffer + 31]
    call to_decimal

    mov rdx, rax
    mov rsi, rdi
    mov eax, 1
    mov edi, 1
    syscall

    dec r12
    jnz .failure
    xor edi, edi
    jmp .exit
.failure:
    mov edi, 1
.exit:
    mov eax, 60
    syscall

; in:  rax = value, rdi = one past the end of the buffer
; out: rdi = first digit, rax = number of digits
to_decimal:
    mov rcx, rdi
    mov r8d, 10
.digit:
    xor edx, edx
    div r8
    add dl, '0'
    dec rdi
    mov [rdi], dl
    test rax, rax
    jnz .digit
    mov rax, rcx
    sub rax, rdi
    ret

section .bss
buffer resb 32
