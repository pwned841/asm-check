; What asm-check can improve once it understands what the code does.
global process

section .text
; rdi = text length, rsi = digit, r15 = write offset
process:
    ; four byte stores that spell a word
    mov byte [buffer + r15], 'H'
    mov byte [buffer + r15 + 1], '4'
    mov byte [buffer + r15 + 2], 'C'
    mov byte [buffer + r15 + 3], 'K'

    ; a division by a constant power of two
    mov r8, 8
    mov rax, rdi
    xor edx, edx
    div r8

    ; turning a digit above 9 into a letter
    sub rdx, 10
    add rdx, 'A'

    ; a copy followed by an addition
    mov rax, r15
    add rax, 4
    cmp rax, rsi
    ret

section .bss
buffer resb 64
