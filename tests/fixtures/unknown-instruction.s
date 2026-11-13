section .text
f:
    mvo rax, 1              ;~
    sycall                  ;~
    pus rax                 ;~
    jmpp f                  ;~
    movq xmm0, rax
    por xmm0, xmm1
    vaddps ymm0, ymm1, ymm2
    andn rax, rbx, rcx
    weird rax
    ret
