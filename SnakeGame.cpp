#include "SnakeGame.h"

SnakeGame::SnakeGame() : rng(std::random_device{}()) {
    reset();
}

void SnakeGame::reset() {
    snake.clear();
    snake.push_back({10, 8});
    snake.push_back({9, 8});
    snake.push_back({8, 8});
    dir = Direction::RIGHT;
    nextDir = Direction::RIGHT;
    gameOver = false;
    score = 0;
    stepDelayMs = 180;
    spawnFood();
    updateFrameBuffer();
}

void SnakeGame::setDirection(Direction d) {
    if (dir == Direction::UP && d == Direction::DOWN) return;
    if (dir == Direction::DOWN && d == Direction::UP) return;
    if (dir == Direction::LEFT && d == Direction::RIGHT) return;
    if (dir == Direction::RIGHT && d == Direction::LEFT) return;
    nextDir = d;
}

void SnakeGame::spawnFood() {
    if (snake.size() >= static_cast<size_t>(WIDTH * HEIGHT)) {
        gameOver = true;
        return;
    }
    std::uniform_int_distribution<int> distX(0, WIDTH - 1);
    std::uniform_int_distribution<int> distY(0, HEIGHT - 1);
    while (true) {
        Point p = {distX(rng), distY(rng)};
        bool onSnake = false;
        for (const auto& body : snake) {
            if (body == p) {
                onSnake = true;
                break;
            }
        }
        if (!onSnake) {
            food = p;
            break;
        }
    }
}

void SnakeGame::update() {
    if (gameOver) return;

    dir = nextDir;
    Point head = snake.front();

    switch (dir) {
        case Direction::UP:    head.y--; break;
        case Direction::DOWN:  head.y++; break;
        case Direction::LEFT:  head.x--; break;
        case Direction::RIGHT: head.x++; break;
    }

    // 1. 벽 충돌 판정
    if (head.x < 0 || head.x >= WIDTH || head.y < 0 || head.y >= HEIGHT) {
        gameOver = true;
        return;
    }

    // 2. 먹이 섭취 여부 확인 및 자기 몸통 충돌 판정
    bool ateFood = (head == food);
    size_t checkLimit = ateFood ? snake.size() : snake.size() - 1;

    for (size_t i = 0; i < checkLimit; ++i) {
        if (head == snake[i]) {
            gameOver = true;
            return;
        }
    }

    snake.push_front(head);

    if (ateFood) {
        score += 10;
        if (stepDelayMs > 60) stepDelayMs -= 8;
        spawnFood();
    } else {
        snake.pop_back();
    }

    updateFrameBuffer();
}

void SnakeGame::updateFrameBuffer() {
    for (int y = 0; y < HEIGHT; ++y) {
        for (int x = 0; x < WIDTH; ++x) {
            frameBuffer[y][x] = false;
        }
    }
    frameBuffer[food.y][food.x] = true;
    for (const auto& body : snake) {
        frameBuffer[body.y][body.x] = true;
    }
}

bool SnakeGame::getPixel(int x, int y) const { return frameBuffer[y][x]; }
bool SnakeGame::isGameOver() const { return gameOver; }
int SnakeGame::getScore() const { return score; }
int SnakeGame::getStepDelayMs() const { return stepDelayMs; }
Point SnakeGame::getFood() const { return food; }
Point SnakeGame::getHead() const { return snake.front(); }
