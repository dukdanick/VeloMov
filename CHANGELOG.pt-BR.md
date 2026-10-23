# Novidades

Todas as mudanças importantes do VeloMov. *English version: [CHANGELOG.md](./CHANGELOG.md).*

## 0.9.0 — 23/10/2026

### Fontes

* **Prévia das fontes**: a lista de fontes mostra o nome de cada fonte escrito nela mesma (com o
  mesmo motor de texto do vídeo, então o que você vê é o que vai sair). Acima da lista, uma prévia
  grande mostra o texto do próprio clipe na fonte que está debaixo do mouse, com o negrito/itálico
  do clipe.
* **Fontes favoritas**: clique na ☆ ao lado de uma fonte para guardá-la na seção **Favoritas**, no
  topo da lista. A ☆ ao lado da pesquisa mostra só as favoritas.
* **Fontes usadas recentemente**: as últimas 8 fontes escolhidas aparecem em **Usadas
  recentemente**, logo abaixo das favoritas.
* **Configurações → Geral → Fontes de texto** mostra quantas favoritas e recentes existem, permite
  tirar uma favorita e limpar cada lista.
* A lista só desenha as linhas que aparecem na tela, então abre na hora mesmo com milhares de
  fontes.

### Nova tela inicial

* **Banner de boas-vindas**: uma "aurora" que se move devagar nas cores do gradiente, saudação de
  acordo com a hora do dia (bom dia, boa tarde, boa noite), quantos projetos você tem e quanto vídeo
  eles somam, e as ações principais (**Criar projeto**, **Importar mídia**, **Projeto de
  demonstração**).
* Uma pequena linha do tempo animada (régua, clipes e agulha) ao lado do card **Continuar
  editando**, que abre o último projeto editado.
* **Dica do dia** no rodapé do banner (clique para ver a próxima).
* **Seus projetos** podem aparecer em **grade** ou **lista**, ordenados por **editados por último**,
  **nome** ou **duração**. Os cards sobem ao passar o mouse e mostram um botão de play; as linhas da
  lista têm um menu **…**.
* Nova tela vazia: uma área para soltar arquivos que também cria o primeiro projeto ao ser clicada.
* A navegação (**Abrir projeto**, **Configurações**, **Sobre**) foi para uma barra no topo — dá para
  voltar a usar uma barra lateral, à esquerda ou à direita (veja abaixo).
* As datas dizem "ontem" em vez de "1 dias atrás".

### Configurações em abas

* A janela de configurações agora tem abas na lateral esquerda: **Aparência**, **Layout**,
  **Geral**, **Desempenho**, **Legendas** e **Armazenamento**. Cada aba tem uma descrição curta e,
  onde faz sentido, seu próprio botão **Restaurar padrões**.

### Personalize a interface

**Aparência**

* **Gradiente** (botão Exportar, banner da tela inicial e outros destaques):
  * *Automático* — da cor de destaque até uma cor vizinha; o controle **Segunda cor** gira essa cor
    pelo círculo cromático (−180° a 180°).
  * *Duas cores* — quaisquer duas cores, com botão **Inverter**.
  * *Desligado* — sem gradiente, só a cor de destaque.
  * **Direção**: horizontal, diagonal ou vertical. Uma barra de prévia e um botão de exemplo mostram
    o resultado na hora.
* **Cantos arredondados** dos painéis, janelas e botões (0 = quadrado, até 20).
* **Espaço entre painéis** (0–12).
* **Bordas nos painéis**: um contorno fino em volta de cada painel.
* **Texto com alto contraste**: textos secundários mais claros.

**Layout**

* **Tamanho da interface**: de 80 % a 150 % (bom para telas de alta resolução).
* **Editor**: biblioteca de mídia à **esquerda** ou à **direita** (o painel de propriedades vai para o
  outro lado), linha do tempo **embaixo** ou **em cima**, e **altura das faixas** (compacta, normal,
  grande). Um desenho pequeno mostra como fica.
* **Tela inicial**: navegação em uma **barra no topo**, **lateral esquerda** ou **lateral direita**;
  **banner de boas-vindas** ligado ou desligado; projetos em **grade** ou **lista**; **tamanho dos
  cards** (pequeno, normal, grande).
* **Notificações** em cima ou embaixo na janela.
* **Animações** ligadas ou desligadas (menus, cards ao passar o mouse, fundo animado da tela
  inicial).

Tudo vale na hora, sem reiniciar, e fica salvo junto com as outras configurações.

### Compatibilidade

* As configurações da 0.8 abrem com todas as opções novas no padrão. O formato dos projetos não
  mudou.

## 0.8.1 — 22/10/2026 (pré-lançamento)

* Os projetos são salvos como arquivos `.vmov`; projetos `.vcut` da 0.8.0 e anteriores continuam
  abrindo e são renomeados quando possível.
* Projetos salvos em outro lugar com **Salvar como** aparecem na tela inicial; as capas desses
  projetos ficam na pasta de dados do VeloMov, e não ao lado do projeto.
* Os losangos de quadros-chave na linha do tempo aparecem por cima das alças de corte e das
  transições.

## 0.8.0 — 22/10/2026

* O VeloCut agora se chama **VeloMov**.
* **Legendas automáticas animadas** com whisper.cpp, no seu computador (na placa de vídeo via Vulkan
  quando disponível), em 12 estilos, com escolha do modelo nas Configurações.
* Versão para **Windows**: instalador e zip portátil.
* Correção: a alça de transformar do player não começa mais a arrastar de fora do player.
