section .data
msg db 1
    mov rax, 1                  ;~
    ret

section .bss
buf resb 1
    nop                         ;~

section .text
    ret
