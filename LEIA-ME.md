# VeloMov

**Editor de vídeo leve para Linux e Windows.** Feito em Rust, com composição na placa de vídeo,
quadros-chave, transições com borrão de movimento de verdade, efeitos, filtros, textos, adesivos,
legendas automáticas animadas e exportação pelo FFmpeg com o jeito dos editores de vídeo para
redes sociais: biblioteca, player, detalhes e linha do tempo.

*English version: [README.md](./README.md).*

![Editor](docs/editor.png)

## Destaques

| | |
|---|---|
| **Interface familiar** | Biblioteca à esquerda (mídia, áudio, texto, legendas, adesivos, efeitos, transições, filtros, ajuste), player no meio, detalhes à direita, linha do tempo com várias faixas e ímã da faixa principal embaixo. Em português do Brasil e inglês (segue o `LANG`). |
| **Linha do tempo** | Ímã da faixa principal, encaixe automático, vínculo áudio/vídeo, dividir (`Ctrl+B`), modo dividir (`B`), excluir à esquerda/direita (`Q`/`W`), aparar, excluir fechando o espaço, arrastar entre faixas, grupos, marcadores, trecho de entrada/saída, quadro congelado, reverso, extrair áudio, copiar/colar atributos, desfazer ilimitado. |
| **Quadros-chave** | Posição, escala, rotação, opacidade, volume, todos os ajustes, intensidade de filtros/efeitos e máscaras. Losango em cada propriedade, curvas (linear, suavizar entrada/saída, suave, ultrapassar, quicar, segurar), quadros-chave arrastáveis na linha do tempo. |
| **Transições** | 56 transições (Básicas, Câmera, Deslizar, Limpar, Desfoque, Glitch, Luz, Formas, 3D): mistura, fade preto, flash branco, puxar, chicote, deslizar, giro, cubo, virar, porta, glitch, vazamento de luz, coração/estrela/círculo… As de movimento usam **borrão de movimento real** (ajustável). A transição fica centrada no corte, então **nunca muda a duração** do vídeo. |
| **Animações** | Entrada / Saída / Combinação para clipes, textos e adesivos (deslizar, zoom, girar, quicar, pop, cair, máquina de escrever, batimento…) com borrão de movimento. |
| **Efeitos e filtros** | 44 efeitos (desfoque, lente zoom, tremor, glitch, RGB separado, VHS, filme antigo, CRT, meio-tom, olho de peixe, caleidoscópio, brilho, vazamento de luz, tela dividida, neve, chuva, visão noturna…) e 31 filtros de cor. |
| **Ajuste** | Brilho, contraste, saturação, exposição, temperatura, matiz, realces, sombras, brancos, pretos, nitidez, clareza, esmaecer, vinheta, granulação, tom — curvas pensadas para vídeo. |
| **Ferramentas de vídeo** | Gizmo de transformação no player (mover/escalar/girar com encaixe no centro), recorte, fundo desfocado/colorido, 14 modos de mesclagem, máscaras (linear, espelho, círculo, retângulo, coração, estrela) com suavização, chroma key, velocidade 0,1×–100× e curvas de velocidade (Montagem, Herói, Bala, Corte seco, Flash de entrada/saída, Velocity). |
| **Áudio** | Forma de onda, volume (dB) com quadros-chave, fade de entrada/saída, remover silêncios, reduzir ruído, efeitos de voz, efeitos sonoros sintetizados (whoosh, chicote, pop, boom, riser…), mudança de velocidade mantendo o tom. |
| **Legendas automáticas** | Reconhecimento de fala com whisper.cpp, no seu computador (na placa de vídeo via Vulkan quando dá). Legendas com o tempo de cada palavra em 12 estilos animados — karaokê, destaque, caixa na palavra, pop, quicar, revelar, uma palavra, brilho, sublinhado, holofote, balão, clássica — trocados para todas de uma vez. Modelo de fala à escolha (tiny … large-v3, baixado no primeiro uso), explicado nas Configurações. Corrigir uma palavra mantém o tempo. |
| **Texto e adesivos** | Qualquer fonte do sistema, contorno, caixa de fundo, sombra, espaçamento de letras/linhas, predefinições; adesivos vetoriais e emoji. |
| **Exportar** | H.264, HEVC, AV1, VP9, ProRes, GIF, codificação na GPU (VA-API), 480p–4K, 24–60 fps, só o áudio (MP3/AAC/WAV/FLAC), trecho de entrada/saída. A exportação usa o mesmo compositor da prévia. |

## 1. Instalar

### Linux

Requisitos: Rust 1.80+, FFmpeg 6+ (`ffmpeg` e `ffprobe` no `PATH`), placa de vídeo com Vulkan ou OpenGL.

```bash
# Arch / CachyOS / Manjaro
sudo pacman -S --needed rust ffmpeg shaderc
# Debian / Ubuntu
sudo apt install cargo build-essential ffmpeg libasound2-dev pkg-config curl glslc

cd velomov            # a pasta do código (aqui: ~/velomov)
./install.sh          # compila a versão otimizada e instala em ~/.local (sem root)
```

O `install.sh` também compila o **whisper.cpp** (reconhecimento de fala das legendas automáticas) em
`~/.local/lib/velomov/whisper/`, usando a placa de vídeo via **Vulkan** quando o `glslc` (pacote
`shaderc`) está instalado; sem ele, usa só o processador. Para refazer só essa parte:
`packaging/linux/build-whisper.sh`.

Para remover: `./uninstall.sh` (os projetos e os modelos de fala baixados são mantidos).

### Windows

Os arquivos prontos ficam em `dist/`:

* `VeloMov-0.8.0-windows-x64-setup.exe` — **instalador** (em português; atalhos no Menu Iniciar e na
  Área de Trabalho, abre arquivos `.vcut`, desinstala pelo Windows). Não precisa de administrador.
* `VeloMov-0.8.0-windows-x64-portable.zip` — **portátil**: descompacte em qualquer pasta/pendrive e
  abra `velomov.exe`. Projetos, configurações e cache ficam na pasta `data` ao lado do programa
  (o arquivo `portable.txt` liga isso; apague-o para usar `%APPDATA%\VeloMov`).

Os dois já trazem o FFmpeg e o whisper.cpp (GPU via Vulkan, com o processador como reserva; não
precisa do Visual C++). Precisa de Windows 10 ou 11 de 64 bits.

**Gerar de novo** (depois de mudar o código), no Linux:

```bash
packaging/windows/build-cross.sh                    # instalador + portátil em dist/
SKIP_INSTALLER=1 packaging/windows/build-cross.sh   # só o portátil
```

Na primeira vez ele baixa o que precisa em `~/.cache/velomov-cross` (Rust para Windows, llvm-mingw,
FFmpeg, whisper.cpp e o Inno Setup, que roda pelo wine) — não precisa de root. Depois de gerar, essa
pasta pode ser apagada; os arquivos de `dist/` continuam funcionando.

**Compilar no próprio Windows**: instale o Rust (https://rustup.rs) e, no PowerShell, na pasta do
código: `powershell -ExecutionPolicy Bypass -File packaging\windows\build-windows.ps1`
(o instalador sai se o Inno Setup 6 estiver instalado).

## 2. Abrir

* Pelo menu de aplicativos: **VeloMov** (categoria Multimídia), ou no terminal: `velomov`
* `velomov projeto.vcut` abre um projeto; `velomov video1.mp4 video2.mp4` cria um projeto com os
  arquivos (entram na faixa principal, em ordem)
* sem instalar: `./run.sh`

Os projetos são salvos sozinhos em `~/.local/share/velomov/projects` (Windows:
`%APPDATA%\VeloMov\projects`); o cache fica em `~/.cache/velomov` (Windows: `%LOCALAPPDATA%\VeloMov`).

## 3. Editar

1. **Criar projeto** na tela inicial (ou **Projeto de demonstração** para ver tudo funcionando).
   Abre a janela de novo projeto: **nome** (padrão "Novo Projeto"), **proporção** (16:9, 9:16, 1:1,
   4:5… *Automática* = igual ao primeiro vídeo, ou *Personalizada* com qualquer largura × altura),
   **resolução** (480p a 4K), **taxa de quadros** (24–60) e **cor de fundo**. Pode deixar tudo como
   está e clicar em **Criar projeto** (ou Enter). Tudo isso dá para mudar depois no painel
   **Detalhes** (clique num espaço vazio da linha do tempo).
2. **Importar**: arraste vídeos/fotos/músicas do gerenciador de arquivos para a janela, ou `Ctrl+I`.
   Num projeto vazio eles já entram na linha do tempo, na ordem. Depois disso, vídeos/fotos soltos
   na janela vão para o fim da faixa principal e **músicas vão para o cursor, numa faixa de áudio**
   (`Ctrl+Z` desfaz).
3. Arraste a mídia da aba **Mídia** para a linha do tempo (vídeos, fotos **e músicas** aparecem lá).
   A faixa de baixo é a **faixa principal** (com ímã: os clipes ficam colados). Solte acima dela para
   criar uma **camada** (sobreposição); música solta em qualquer lugar vai para uma faixa de áudio.
4. **Vários clipes de uma vez**: `Shift`+clique seleciona **do último clipe clicado até o clicado**
   (todos no meio); `Ctrl`+clique adiciona/tira um por um. Com vários selecionados, tudo vale para
   todos: arrastar a borda muda o tamanho de todos, arrastar move todos, o painel da direita
   (escala, opacidade, animações, ajustes, volume, velocidade…) altera todos, e efeitos, filtros,
   ajustes e transições clicados na biblioteca são aplicados em cada um. A linha do tempo tem barras
   de rolagem à direita (faixas) e embaixo (tempo).
5. **Dividir**: posicione o cursor e aperte `Ctrl+B`. `Q` apaga o que está à esquerda do cursor,
   `W` o que está à direita.
6. **Transições**: clique no **+** que aparece entre dois clipes da faixa principal e escolha na aba
   **Transições** (ou arraste a transição até o corte). Clicando na transição você ajusta a duração
   e o **borrão de movimento** (slide, puxar para dentro, chicote, giro…). A transição fica centrada
   no corte, então **o vídeo não fica mais curto** quando você adiciona uma. Projetos antigos com
   transições são convertidos sozinhos ao abrir, sem mudar a duração nem tirar nada do lugar.
7. **Efeitos / Filtros / Texto / Adesivos / Ajuste**: clique em um item na biblioteca (vai para o
   cursor) ou arraste até a linha do tempo. Passe o mouse sobre um efeito/transição para ver a prévia.
8. **Propriedades**: selecione um clipe — o painel da direita tem Vídeo (transformar, mesclar,
   borrão de movimento, tela, recorte, máscara, remover fundo), Áudio, Velocidade (incluindo curvas
   como Montagem/Herói/Bala), Animação (entrada/saída/combinação) e Ajustar (brilho, contraste…).
9. **Quadros-chave**: clique no losango ◇ ao lado de qualquer propriedade (ou `Alt+K`), mova o
   cursor e mude o valor — um novo quadro-chave é criado sozinho. No player dá para arrastar,
   redimensionar pelos cantos e girar pela alça de baixo (só dentro da área do player).
10. **Exportar**: botão **Exportar** (`Ctrl+E`) → resolução (até 4K), codec (H.264, HEVC, AV1, VP9,
    ProRes, GIF ou GPU/VA-API), taxa de quadros, ou só o áudio. O arquivo vai para a pasta Vídeos
    (dá para trocar).

Na tela inicial, **Selecionar** (ou `Ctrl`+clique, ou o círculo no canto de um projeto) marca
vários projetos de uma vez para **Duplicar** ou **Excluir**; `Ctrl+A` marca todos.

GIF e WebP funcionam como fotos (1 quadro) ou como vídeo (animados), mantendo a transparência.

### 3a. Legendas automáticas animadas

1. Aba **Legendas** na biblioteca (à esquerda) → **Legendas automáticas**.
2. Escolha o **idioma falado** (ou *Detectar automaticamente*), se é para ouvir a **linha do tempo
   inteira** ou só os **clipes selecionados**, o **tamanho da legenda** (Automática, Uma palavra,
   Curta, Média, Longa) e o **estilo**.
3. **Gerar legendas**. Na primeira vez o modelo de fala é baixado (Base = 142 MB, só uma vez).
   As legendas entram numa faixa de texto nova, cada uma com o tempo de cada palavra.

* **Trocar o estilo**: aba **Legendas → Estilos** (passe o mouse para ver a animação). Clicar muda
  **todas** as legendas automáticas; com um texto selecionado, muda só ele. Estilos: Karaokê, Destaque,
  Caixa na palavra, Pop, Quicar, Revelar, Uma palavra, Brilho, Sublinhado, Holofote, Balão e Clássica.
* No painel **Texto** da legenda (direita) há **Animação da legenda** (estilo e cor de destaque) e
  **Aplicar visual a todas as legendas** (estilo, cores, fonte, tamanho e posição). Fonte, contorno,
  fundo e sombra funcionam como em qualquer texto — e qualquer texto comum pode ganhar animação.
* **Corrigir uma palavra**: edite o texto da legenda; o tempo das palavras é mantido (o painel mostra
  "Agora: …" com a palavra falada no cursor). Dividir e aparar legendas mantém a sincronia.
* **Modelo de fala** (Configurações → *Legendas automáticas*), cada um explicado na tela:

| Modelo | Tamanho | Para quê |
|---|---|---|
| Tiny | 78 MB | o mais rápido; erra mais (principalmente em português e com ruído) |
| **Base** (padrão) | 142 MB | equilíbrio: rápido e bom para a maioria dos vídeos |
| Small | 488 MB | mais preciso em português, sotaques e áudio com ruído; ~3× mais lento |
| Medium | 1,5 GB | muito preciso; lento no processador, uns 2 GB de memória |
| Large Turbo | 574 MB | quase o Large, bem mais rápido — ótimo para PCs fortes |
| Large | 3,1 GB | o mais preciso; muito lento sem GPU forte, uns 4 GB de memória |

  Ali também dá para baixar/apagar modelos, escolher o número de threads e ligar/desligar **Usar a
  placa de vídeo**. Numa Vega 11, com GPU, o Base reconhece ~11 s de fala em ~2 s.
* Tudo roda no seu computador; nada é enviado para a internet (só o download do modelo).

### 3b. Áudio: silêncios e ruído

Selecione um clipe com som (vídeo ou música) e abra a aba **Áudio** à direita:

* **Remover silêncios**: ajuste *Abaixo de* (volume considerado silêncio, padrão −40 dB),
  *Duração mín.* (silêncios mais curtos ficam) e *Margem* (respiro mantido antes/depois da fala).
  A linha mostra quantos trechos e quantos segundos sairão; clique em **Remover**. O clipe é cortado
  e o resto da faixa sobe para fechar os buracos (`Ctrl+Z` desfaz). Também no clique direito do
  clipe → *Remover silêncios*.
* **Reduzir ruído**: liga a redução de chiado, zumbido, ventilador e vento; *Intensidade* de 10 a 100%.
  Vale na prévia e na exportação.

### 3c. Qualidade da prévia e desempenho

No canto superior direito do **Player** há o botão de qualidade (ícone de velocímetro, **Média** por padrão):

| Qualidade | O que faz |
|---|---|
| **Alta** | usa os arquivos originais, na resolução máxima |
| **Média** | usa **proxies** (cópias leves em 540p) dos vídeos pesados e renderiza até 720p |
| **Baixa** | proxies, até 480p e efeitos simplificados — a mais leve |

* Vídeos pesados (4K, HEVC/H.265, AV1, VP9, ProRes, acima de 1080p30) ganham um proxy
  automaticamente, feito em segundo plano. Na aba **Mídia** aparece ⚡ com a porcentagem enquanto ele
  é criado e só ⚡ quando ficou pronto; o Player mostra "Otimizando mídia". A criação pausa sozinha
  enquanto você dá play. A **exportação sempre usa os arquivos originais** em qualidade máxima.
* A decodificação por hardware vem ligada (VA-API no Linux, D3D11/DXVA no Windows); se um arquivo não
  for suportado pela placa, o VeloMov troca sozinho para o processador só para aquele arquivo. Quando
  o vídeo tem mais quadros do que a prévia mostra (60 fps num projeto de 30), os que sobram nem são
  decodificados.
* A interface nunca espera a decodificação: decodificadores em segundo plano com pré-carregamento
  antes de cortes e transições, e nunca um quadro preto enquanto um vídeo busca a posição.
* Decodifica no tamanho mostrado (¼, ½ ou inteiro); conversão de cor, ajustes, máscaras, mesclagem,
  transições e borrão de movimento rodam na placa de vídeo (wgpu / Vulkan).
* Um programa só (~20 MB). Parado: **0 % de CPU**, ~130 MB de RAM. Medido numa Vega 11 com vídeo
  **4K60 HEVC 10 bits** de celular e transição chicote: nenhum quadro atrasado em qualquer qualidade.

### 3d. Configurações

Na tela inicial clique em **Configurações**, ou no editor use **Menu → Configurações**:

* **Aparência**: tema (Preto — padrão —, Preto puro, Grafite, Azul meia-noite, Ameixa) e **cor de
  destaque** (8 cores prontas ou qualquer cor em *Personalizada*). Muda na hora, sem reiniciar.
* **Geral**: idioma, duração das fotos, salvamento automático.
* **Desempenho**: qualidade da prévia, decodificação por hardware, proxies.
* **Legendas automáticas**: modelo de fala (com explicação de cada um), GPU, threads.
* **Armazenamento**: o cache (áudio extraído, proxies, miniaturas) é **apagado sozinho** quando um
  arquivo fica **7 dias sem uso** — dá para trocar para 1, 3, 14 ou 30 dias, ou *Nunca*. Mostra o
  tamanho atual e tem o botão **Limpar cache agora**. Nada se perde: o VeloMov recria o que precisar.

## 4. Atalhos

| Tecla | Ação | Tecla | Ação |
|---|---|---|---|
| `Espaço` | reproduzir / pausar | `Ctrl+B` | dividir o clipe selecionado |
| `J` `K` `L` | voltar / pausar / avançar (de novo: 2×, 4×) | `Ctrl+Shift+B` | dividir todas as faixas |
| `←` `→` | quadro anterior / próximo | `Q` / `W` | excluir à esquerda / direita do cursor |
| `↑` `↓` | corte anterior / próximo | `B` / `A` | modo dividir / modo seleção |
| `Home` `End` | início / fim | `Delete` · `Shift+Delete` | excluir · excluir e fechar o espaço |
| `Alt+K` | pôr / tirar quadro-chave | `Alt+[` `Alt+]` | quadro-chave anterior / próximo |
| `P` | ímã da faixa principal | `N` | encaixe automático |
| `~` | vínculo áudio/vídeo | `S` | eixo de prévia |
| `F` | quadro congelado | `Ctrl+Shift+R` | reverso |
| `Ctrl+Shift+S` | extrair áudio | `R` | girar 90° |
| `Ctrl+G` / `Ctrl+Shift+G` | agrupar / desagrupar | `Ctrl+D` | duplicar |
| `M` | marcador | `I` / `O` | entrada / saída (exportar só um trecho) |
| `Ctrl+E` | exportar | `Ctrl+Shift+F` | prévia em tela cheia |
| `Ctrl+Z` / `Ctrl+Shift+Z` | desfazer / refazer | `?` ou `F1` | lista completa |

## 5. Se algo der errado

* **Sem som**: o VeloMov usa a saída padrão (PipeWire/ALSA no Linux). Rode `velomov` no terminal e
  veja a linha `audio output: …`.
* **Arrastar arquivos não funciona** (Linux): o VeloMov abre via XWayland para aceitar arrastar e
  soltar. Se preferir Wayland nativo: `VELOMOV_WAYLAND=1 velomov` (aí use `Ctrl+I` para importar).
* **Prévia travando**: use a qualidade **Média** ou **Baixa** no Player e espere o ⚡ do vídeo
  ficar pronto. Confira em Configurações se a *Decodificação por hardware* está ligada.
* **Legendas falham**: em Configurações → Legendas automáticas, desligue *Usar a placa de vídeo*.
  Se aparecer "reconhecimento de fala não instalado" no Linux, rode `packaging/linux/build-whisper.sh`.
* **Placa de vídeo**: precisa de Vulkan, DirectX 12 (Windows) ou OpenGL; para forçar OpenGL:
  `WGPU_BACKEND=gl velomov`.
* Limpar caches (áudio, clipes invertidos, proxies e miniaturas): Configurações → Armazenamento.

## 6. Para desenvolvedores

```
src/
  model/      projeto, faixas, clipes, quadros-chave, operações de edição (dividir, aparar, ímã…), catálogos
  engine/     compositor wgpu: camadas, borrão de movimento, transições, efeitos, cor (shaders.wgsl), legendas animadas
  media/      ffprobe, decodificadores FFmpeg, cache de áudio + mixer, miniaturas, proxies, efeitos sonoros, transcrição (whisper)
  app/        interface egui: início, biblioteca, player + gizmo, inspetor, linha do tempo, atalhos, exportação, legendas
  export.rs   render na GPU → FFmpeg, mixagem de áudio
  playback.rs saída de áudio em tempo real (cpal); o relógio do áudio move o cursor
packaging/    .desktop do Linux, build do whisper, build/instalador do Windows
```

* **Borrão de movimento**: o shader de composição integra a transformação de cada camada ao longo de
  um obturador virtual (início / meio / fim do quadro), com mais amostras quanto maior o deslocamento.
* **Legendas**: o whisper-cli gera uma palavra por segmento (`-ml 1 -sow`); as palavras viram linhas
  e cada estilo calcula, a cada quadro, cor, escala, deslocamento e opacidade de cada palavra.
* **Efeitos sonoros** são sintetizados: passagens de ar com ruído rosa, efeito Doppler, turbulência e
  panorâmica, mais um pequeno reverb Schroeder.

```bash
velomov --make-demo PASTA               # gera mídia de exemplo e PASTA/demo.vcut
velomov --new-project P.vcut a.mp4 b.mp4
velomov --render-frame P.vcut 2.5 out.png --height 720
velomov --export P.vcut out.mp4 --res 1080 --codec hevc --quality higher
velomov --captions P.vcut --lang pt --style karaoke [--model base] [--no-gpu]
velomov --catalog-sheet PASTA           # folhas com todas as transições/efeitos/filtros/animações
VELOMOV_SCRIPT=teste.txt velomov P.vcut # repete entrada da interface (veja src/app/script.rs)
VELOMOV_STATS=1 velomov                 # estatísticas de quadros atrasados
VELOMOV_DEBUG_DECODE=1 velomov          # registra reinícios do decodificador
```

Variáveis: `VELOMOV_WAYLAND=1` força Wayland nativo, `VELOMOV_NO_AUDIO=1` desliga o som,
`VELOMOV_NO_HW=1` desliga a decodificação por hardware, `VELOMOV_WHISPER=/caminho/whisper-cli`
escolhe o whisper, `WGPU_BACKEND=gl` usa OpenGL.

## Licença

GPL-3.0-or-later. Veja [LICENSE](LICENSE).
