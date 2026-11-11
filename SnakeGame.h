#pragma once
#include <deque>
#include <random>

enum class Direction { UP, DOWN, LEFT, RIGHT };

struct Point {
    int x, y;
    bool operator==(const Point& other) const {
        return x == other.x && y == other.y;
    }
};

// 32x16 뱀 게임 엔진 클래스 (선언부)
class SnakeGame {
public:
    static const int WIDTH = 32;
    static const int HEIGHT = 16;

private:
    std::deque<Point> snake;
    Point food;
    Direction dir;
    Direction nextDir;
    bool gameOver;
    int score;
    int stepDelayMs;
    bool frameBuffer[HEIGHT][WIDTH];
    std::mt19937 rng;

public:
    SnakeGame();

    void reset();
    void setDirection(Direction d);
    void spawnFood();
    void update();
    void updateFrameBuffer();

    bool getPixel(int x, int y) const;
    bool isGameOver() const;
    int getScore() const;
    int getStepDelayMs() const;
    Point getFood() const;
    Point getHead() const;
};
